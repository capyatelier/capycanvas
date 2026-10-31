//! Compact GTK layer list. Virtual rows render shared state and typed commands.
use crate::{number_control::NumberControl, workspace::Workspace};
use gtk::{gdk, gio, glib, prelude::*};
use layer_render::CanvasRenderer;
use layer_ui::{LayerAction as A, LayerState, NumericControl, UiAction, UiState};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};
mod connections;
mod touch;

struct LayerCopy {
    layer: layer_ui::native_copy::LayerCopy,
    choose_texture: std::sync::Arc<str>,
    reference: std::sync::Arc<str>,
    load_selection: std::sync::Arc<str>,
}
impl LayerCopy {
    fn new(localization: &layer_ui::Localizer) -> Self {
        let copy = layer_ui::NativeCopy::new(localization);
        Self {
            layer: copy.layers,
            choose_texture: copy.paper.choose,
            reference: localization.text(layer_ui::MessageId::COMMAND_SELECTION_REFERENCE),
            load_selection: localization.text(layer_ui::MessageId::RESOURCES_SELECTION_MENU_LOAD_SELECTION),
        }
    }
}
fn caption(widget: &impl IsA<gtk::Widget>, text: &str) {
    if widget.as_ref().tooltip_text().as_deref() != Some(text) {
        widget.as_ref().set_tooltip_text(Some(text));
        widget.as_ref().update_property(&[gtk::accessible::Property::Label(text)]);
    }
}

pub struct LayerPanel {
    copy: Rc<RefCell<LayerCopy>>,
    pub root: gtk::Box,
    pub header: gtk::Box,
    pub footer: gtk::Box,
    pub list: gtk::ScrolledWindow,
    pub opacity: NumberControl,
    opacity_popup: gtk::Popover,
    opacity_editor: NumberControl,
    model: gio::ListStore,
    blend: gtk::MenuButton,
    blend_label: gtk::Label,
    blend_menu: gtk::PopoverMenu,
    alpha: gtk::ToggleButton,
    lock: gtk::ToggleButton,
    mask_action: gtk::Button,
    add_filter: gtk::MenuButton,
    unit_row: gtk::Box,
    delete: gtk::Button,
    reference: gtk::ToggleButton,
    attachment: gtk::ToggleButton,
    context: gtk::PopoverMenu,
    owner: Rc<RefCell<Weak<Workspace>>>,
    rows: Rc<RefCell<HashMap<usize, Row>>>,
    connections: connections::Connections,
    previews: Previews,
    requested: RefCell<HashMap<(u64, bool), u64>>,
    pending: RefCell<HashMap<u64, (u64, bool, u64)>>,
    simple: Rc<Cell<bool>>,
    next_preview: Cell<u64>,
    renderer: Cell<u64>,
}
type Previews = Rc<RefCell<HashMap<(u64, bool), (u64, gdk::Texture)>>>;
#[derive(Clone)]
struct Row {
    simple: Rc<Cell<bool>>,
    copy: Rc<RefCell<LayerCopy>>,
    owner: Rc<RefCell<Weak<Workspace>>>,
    previews: Previews,
    id: Cell<u64>,
    bound: Cell<bool>,
    swipe: crate::swipe_row::SwipeRow,
    content_image: gtk::Picture,
    effect_icon: gtk::Image,
    pass_through: gtk::Image,
    mask_image: gtk::Picture,
    root: gtk::Box,
    eye: gtk::Button,
    selection: gtk::Button,
    thumbnails: gtk::Box,
    content: gtk::Button,
    load_selection: gtk::Button,
    mask: gtk::Button,
    link: gtk::Button,
    texture: gtk::Button,
    texture_thumbnail: crate::paper_picker::PaperThumbnail,
    name: gtk::Label,
    name_stack: gtk::Stack,
    name_entry: gtk::Entry,
    meta: gtk::Label,
    lock: gtk::Image,
    grip: gtk::Image,
    pickups: [Pickup; 2],
}
#[derive(Clone)]
struct Pickup {
    source: gtk::DragSource,
    handle: bool,
    dragging: Rc<Cell<Option<layer_ui::PointerKind>>>,
}
impl Pickup {
    fn retire_if_ineligible(&self, row: &LayerState) {
        if self.dragging.get().is_some_and(|kind| !row.can_drag(kind, self.handle, true)) {
            self.source.drag_cancel();
        }
    }
}
fn button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = crate::icons::button(icon);
    b.add_css_class("flat");
    b.add_css_class("layer-icon");
    caption(&b, tooltip);
    b
}
fn action(w: &Rc<Workspace>, action: A) {
    w.dispatch(UiAction::Layer { action });
}
fn row_state(item: &gtk::ListItem) -> Option<LayerState> {
    Some(item.item()?.downcast::<glib::BoxedAnyObject>().ok()?.try_borrow::<LayerState>().ok()?.clone())
}
fn item_id(object: &glib::BoxedAnyObject) -> Option<u64> {
    object.try_borrow::<LayerState>().map(|l| l.id).ok()
}
fn panel_items(layers: &[LayerState], add_paper: Option<&layer_ui::PropertyActionView>) -> Vec<glib::BoxedAnyObject> {
    layers.iter().map(|layer| glib::BoxedAnyObject::new(layer.clone()))
        .chain(add_paper.map(|action| glib::BoxedAnyObject::new(action.clone()))).collect()
}
fn activate_paper(owner: &RefCell<Weak<Workspace>>) {
    let Some(w) = owner.borrow().upgrade() else { return };
    let action = w.gpu.borrow().as_ref().and_then(|g| g.session.state().layer_tools.add_paper.as_ref().map(|a| a.action.clone()));
    if let Some(action) = action { w.dispatch(UiAction::Effect { action }); }
}
fn paper_row(action: &layer_ui::PropertyActionView, owner: Rc<RefCell<Weak<Workspace>>>) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, crate::panel_controls::SPACING);
    content.set_halign(gtk::Align::Center);
    content.set_hexpand(true);
    content.append(&crate::icons::image(action.icon.as_deref().unwrap_or("layer-plus-symbolic")));
    content.append(&gtk::Label::new(Some(&action.label)));
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("layer-row");
    row.set_widget_name("layer-add-paper");
    row.append(&content);
    let click = gtk::GestureClick::new();
    click.set_button(1);
    click.connect_released(glib::clone!(#[weak] row, #[strong] owner, move |gesture, _, x, y| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        if row.contains(x, y) { activate_paper(&owner); }
    }));
    row.add_controller(click);
    row
}
fn thumbnail(tooltip: &str) -> (gtk::Button, gtk::Picture, gtk::Overlay) {
    let button = button("layer-image-symbolic", tooltip);
    button.add_css_class("layer-thumbnail");
    button.set_valign(gtk::Align::Center);
    let picture = gtk::Picture::new();
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_size_request(28, 28);
    picture.set_overflow(gtk::Overflow::Hidden);
    let overlay = gtk::Overlay::new();
    // The 32px asynchronous texture must not participate in measurement.
    // Empty/rebound rows and loaded previews occupy the same 28px square.
    let size = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    size.set_size_request(28, 28);
    overlay.set_child(Some(&size));
    overlay.add_overlay(&picture);
    overlay.set_measure_overlay(&picture, false);
    button.set_child(Some(&overlay));
    (button, picture, overlay)
}
fn row_frame() -> gtk::Box {
    let root=gtk::Box::new(gtk::Orientation::Horizontal,2);
    root.add_css_class("layer-row");
    root.add_css_class("customizable-target");
    root
}
fn row_name() -> gtk::Label {
    let name=gtk::Label::new(None);
    name.add_css_class("layer-name");
    name.set_xalign(0.);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.set_width_chars(1);
    name.set_hexpand(true);
    name
}
fn toggle(icon: &str, tooltip: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::new();
    crate::icons::set_button(&button, icon);
    button.add_css_class("flat");
    button.add_css_class("layer-icon");
    caption(&button, tooltip);
    button
}
fn finish_name(
    owner: &RefCell<Weak<Workspace>>,
    item: &gtk::ListItem,
    stack: &gtk::Stack,
    entry: &gtk::Entry,
    cancel: bool,
) {
    if stack.visible_child_name().as_deref() != Some("edit") {
        return;
    }
    // Hide before dispatch: refresh may rebind this virtual row and move focus.
    let row = row_state(item);
    let name = entry.text().to_string();
    stack.set_visible_child_name("name");
    if let (Some(row), Some(w)) = (row, owner.borrow().upgrade()) {
        // Another row may have started renaming while this entry lost focus.
        if w.gpu
            .borrow()
            .as_ref()
            .is_none_or(|g| g.session.state().layer_tools.rename_layer != Some(row.id))
        {
            return;
        }
        action(
            &w,
            if cancel || name.trim() == row.label || name.trim().is_empty() {
                A::CancelRename
            } else {
                A::Rename { id: row.id, name }
            },
        );
    }
}
fn row_drag(
    item: &gtk::ListItem,
    root: &gtk::Box,
    name: &gtk::Stack,
    handle: bool,
    owner: &Rc<RefCell<Weak<Workspace>>>,
    context: &gtk::PopoverMenu,
) -> Pickup {
    let source = gtk::DragSource::new();
    let contact = Rc::new(Cell::new(None));
    source.set_actions(gdk::DragAction::MOVE);
    source.set_propagation_phase(gtk::PropagationPhase::Capture);
    source.connect_begin(glib::clone!(#[weak] item, #[weak] root, #[strong] contact, move |source, _| {
        let at_rest = root.parent().and_downcast::<crate::swipe_row::SwipeRow>().is_some_and(|row| row.is_at_rest());
        let kind = crate::input::gesture_pointer_kind(source);
        let admitted = row_state(&item).is_some_and(|row| row.can_drag(kind, handle, at_rest));
        contact.set(admitted.then_some(kind));
        if !admitted {
            source.set_state(gtk::EventSequenceState::Denied);
        } else if handle {
            source.set_state(gtk::EventSequenceState::Claimed);
        }
    }));
    let dragging = Rc::new(Cell::new(None));
    source.connect_end(glib::clone!(#[strong] contact, move |_, _| contact.set(None)));
    source.connect_cancel(glib::clone!(#[strong] contact, move |_, _| contact.set(None)));
    source.connect_drag_end(glib::clone!(#[strong] dragging, move |_, _, _| dragging.set(None)));
    source.connect_drag_cancel(glib::clone!(#[strong] dragging, move |_, _, _| { dragging.set(None); false }));
    source.connect_prepare(glib::clone!(
        #[strong]
        contact,
        #[strong]
        dragging,
        #[weak]
        item,
        #[weak]
        root,
        #[weak]
        name,
        #[strong]
        owner,
        #[weak]
        context,
        #[upgrade_or]
        None,
        move |source, x, y| {
            let kind = contact.get()?;
            if name.visible_child_name().as_deref() == Some("edit") {
                return None;
            }
            let row = row_state(&item)?;
            let at_rest = root.parent().and_downcast::<crate::swipe_row::SwipeRow>().is_some_and(|row| row.is_at_rest());
            if !row.can_drag(kind, handle, at_rest) {
                return None;
            }
            let w = owner.borrow().upgrade()?;
            context.popdown();
            context.set_autohide(true);
            let color = w.gpu.borrow().as_ref()?.session.state().palette.panel;
            let preview = drag_preview(root.upcast_ref(), color);
            let hotspot = source
                .widget()?
                .compute_point(&root, &gtk::graphene::Point::new(x as f32, y as f32))?;
            source.set_icon(preview.as_ref(), hotspot.x() as i32, hotspot.y() as i32);
            dragging.set(Some(kind));
            Some(gdk::ContentProvider::for_value(
                &format!("capy-layer:{}", row.id).to_value(),
            ))
        }
    ));
    Pickup { source, handle, dragging }
}
/// Snapshot once at pickup; thumbnail refreshes never mutate the drag image.
pub(crate) fn drag_preview(
    row: &gtk::Widget,
    background: layer_ui::HexColor,
) -> Option<gdk::Paintable> {
    let paintable = gtk::WidgetPaintable::new(Some(row));
    let snapshot = gtk::Snapshot::new();
    snapshot.push_opacity(0.7);
    let [r, g, b] = background.0.map(|v| v as f32 / 255.);
    snapshot.append_color(
        &gdk::RGBA::new(r, g, b, 1.),
        &gtk::graphene::Rect::new(0., 0., row.width() as f32, row.height() as f32),
    );
    paintable.snapshot(&snapshot, row.width() as f64, row.height() as f64);
    snapshot.pop();
    let node = snapshot.to_node()?;
    let snapshot = gtk::Snapshot::new();
    let scale = crate::squircle::device_scale(row);
    snapshot.append_node(crate::squircle::converted(&node, scale));
    snapshot.to_paintable(Some(&gtk::graphene::Size::new(
        row.width() as f32,
        row.height() as f32,
    )))
}
fn dragged_layer(value: &glib::Value) -> Option<u64> {
    value.get::<String>().ok()?.strip_prefix("capy-layer:")?.parse().ok()
}
fn drop_hit(root: &gtk::Box, content: &gtk::Button, x: f64, y: f64) -> (f32, layer_ui::LayerDropSurface) {
    let thumbnail = content.compute_bounds(root).is_some_and(|rect| rect.contains_point(&gtk::graphene::Point::new(x as f32, y as f32)));
    ((y / root.height().max(1) as f64) as f32, if thumbnail { layer_ui::LayerDropSurface::Thumbnail } else { layer_ui::LayerDropSurface::Row })
}
fn show_drop_hint(rows: &RefCell<HashMap<usize, Row>>, hint: Option<layer_ui::LayerDropHint>) {
    for row in rows.borrow().values() {
        for class in ["layer-drop-before", "layer-drop-after", "layer-drop-into"] { row.root.remove_css_class(class); }
        row.content.remove_css_class("layer-drop-attach");
        if hint.is_some_and(|hint| hint.effect_owner == Some(row.id.get())) { row.content.add_css_class("layer-drop-attach"); }
        if let Some(hint) = hint.filter(|hint| hint.target == row.id.get()) {
            use layer_ui::LayerDropPosition as P;
            match hint.position {
                P::Attach => row.content.add_css_class("layer-drop-attach"),
                P::Above => row.root.add_css_class("layer-drop-before"),
                P::Below => row.root.add_css_class("layer-drop-after"),
                P::Into => row.root.add_css_class("layer-drop-into"),
            }
        }
    }
}
fn row_button_action(row: &LayerState, kind: u8) -> UiAction {
    if kind == 5 {
        return UiAction::Selection { action: layer_ui::SelectionAction::LoadLayer { id: row.id, mode: layer_ui::SelectionMode::New, inverted: false } };
    }
    if kind == 0 {
        return UiAction::SetLayerVisibility {
            id: row.id,
            visible: !row.visible,
        };
    }
    UiAction::Layer {
        action: match kind {
            1 => A::SelectRow { id: row.id, extend: false, toggle: true },
            2 if row.group => A::Collapse { id: row.id },
            2 => A::Select {
                id: row.id,
                mask: false,
            },
            3 => A::LinkMask {
                id: row.id,
                value: !row.mask_linked,
            },
            _ => A::Select {
                id: row.id,
                mask: true,
            },
        },
    }
}
impl LayerPanel {
    pub fn document_changed(&self) {
        self.previews.borrow_mut().clear();
        self.requested.borrow_mut().clear();
        self.pending.borrow_mut().clear();
        for row in self.rows.borrow().values() {
            row.content_image.set_paintable(None::<&gdk::Texture>);
            row.mask_image.set_paintable(None::<&gdk::Texture>);
        }
        self.model.remove_all();
    }
    #[cfg(test)]
    pub fn preview_requests(&self) -> u64 {
        self.next_preview.get() - 1
    }
    #[cfg(test)]
    pub(super) fn preview_texture(&self, id: u64, revision: u64, extra: &[Rc<Self>]) -> Option<gdk::Texture> {
        let previews = self.previews.borrow();
        let (current, texture) = previews.get(&(id, false))?;
        if *current != revision { return None; }
        std::iter::once(self).chain(extra.iter().map(|v| v.as_ref()))
            .filter(|view| view.root.is_mapped())
            .find_map(|view| view.rows.borrow().values().find(|row| row.id.get() == id)
                .and_then(|row| row.content_image.paintable())
                .and_then(|paintable| paintable.downcast::<gdk::Texture>().ok())
                .filter(|visible| visible == texture))
    }
    #[cfg(test)]
    pub(super) fn preview_debug(&self, extra: &[Rc<Self>]) -> serde_json::Value {
        serde_json::json!({
            "pending": self.pending.borrow().iter().collect::<Vec<_>>(),
            "requested": self.requested.borrow().iter().collect::<Vec<_>>(),
            "cached": self.previews.borrow().iter().map(|(key,(revision,_))| (key, revision)).collect::<Vec<_>>(),
            "views": std::iter::once(self).chain(extra.iter().map(|v| v.as_ref())).map(|view| serde_json::json!({
                "mapped": view.root.is_mapped(),
                "rows": view.rows.borrow().values().map(|row| serde_json::json!({
                    "id": row.id.get(), "mapped": row.root.is_mapped(),
                    "paintable": row.content_image.paintable().is_some(),
                })).collect::<Vec<_>>()
            })).collect::<Vec<_>>()
        })
    }
    pub fn new(localization: std::sync::Arc<layer_ui::Localizer>) -> Self {
        Self::with_previews(localization, Previews::default())
    }
    pub fn sharing(&self, localization: std::sync::Arc<layer_ui::Localizer>) -> Self {
        let view = Self::with_previews(localization, self.previews.clone());
        view.root.set_margin_top(layer_ui::PANEL_CONTROL_METRICS.compound_spacing as i32);
        view.root.set_margin_bottom(layer_ui::PANEL_CONTROL_METRICS.compound_spacing as i32);
        view
    }
    fn with_previews(localization: std::sync::Arc<layer_ui::Localizer>, previews: Previews) -> Self {
        let copy = Rc::new(RefCell::new(LayerCopy::new(&localization)));
        let owner: Rc<RefCell<Weak<Workspace>>> = Rc::default();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("layers-panel");
        let context = gtk::PopoverMenu::from_model(None::<&gio::Menu>);
        context.set_parent(&root);
        context.set_has_arrow(false);
        let opacity_editor = NumberControl::panel(NumericControl::layer_opacity(), copy.borrow().layer.opacity.as_ref(), localization.clone());
        opacity_editor.set_widget_name("layer-gesture-opacity");
        opacity_editor.set_size_request(220, -1);
        opacity_editor.set_margin_start(12); opacity_editor.set_margin_end(12);
        opacity_editor.set_margin_top(12); opacity_editor.set_margin_bottom(12);
        let opacity_popup = gtk::Popover::new();
        opacity_popup.set_child(Some(&opacity_editor));
        opacity_popup.set_parent(&root);
        let release = gtk::EventControllerLegacy::new();
        release.set_propagation_phase(gtk::PropagationPhase::Capture);
        release.connect_event(glib::clone!(
            #[weak]
            context,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                if matches!(
                    event.event_type(),
                    gdk::EventType::TouchEnd
                        | gdk::EventType::TouchCancel
                        | gdk::EventType::ButtonRelease
                ) && context.is_visible()
                    && !context.is_autohide()
                {
                    context.popdown();
                    context.set_autohide(true);
                    if event.event_type() != gdk::EventType::TouchCancel {
                        context.popup();
                    }
                }
                glib::Propagation::Proceed
            }
        ));
        root.add_controller(release);
        let header = gtk::Box::new(gtk::Orientation::Vertical, 2);
        header.add_css_class("layer-header");
        root.append(&header);
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        options.set_homogeneous(true);
        let blend = gtk::MenuButton::new();
        let blend_label = gtk::Label::new(Some(localization.text(layer_ui::MessageId::RESOURCES_BLEND_NORMAL).as_ref()));
        blend_label.set_xalign(0.);
        blend_label.set_hexpand(true);
        blend_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        blend_label.set_max_width_chars(10);
        blend_label.set_width_chars(1);
        blend.set_child(Some(&blend_label));
        blend.set_always_show_arrow(true);
        blend.set_hexpand(true);
        blend.set_widget_name("layer-blend");
        caption(&blend, copy.borrow().layer.blend.as_ref());
        let blend_menu = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
        blend.set_popover(Some(&blend_menu));
        options.append(&blend);
        let opacity = NumberControl::inline(NumericControl::layer_opacity(), copy.borrow().layer.opacity.as_ref(), localization.clone());
        options.append(&opacity);
        header.append(&options);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let alpha = toggle("layer-alpha-lock-symbolic", copy.borrow().layer.alpha_lock.as_ref());
        let lock = toggle("layer-lock-symbolic", copy.borrow().layer.lock_editing.as_ref());
        let attachment = toggle("layer-clip-symbolic", localization.text(layer_ui::MessageId::RESOURCES_LAYER_ATTACH_UNAVAILABLE).as_ref());
        attachment.set_widget_name("layer-attachment");
        let reference = toggle(
            "layer-reference-symbolic",
            copy.borrow().reference.as_ref(),
        );
        reference.add_css_class("layer-reference");
        for b in [&alpha, &lock, &attachment, &reference] {
            actions.append(b);
        }
        header.append(&actions);
        let model = gio::ListStore::new::<glib::BoxedAnyObject>();
        let factory = gtk::SignalListItemFactory::new();
        let rows: Rc<RefCell<HashMap<usize, Row>>> = Rc::default();
        let simple: Rc<Cell<bool>> = Rc::default();
        let connections = connections::Connections::new(rows.clone());
        factory.connect_setup(glib::clone!(
            #[weak]
            connections,
            #[strong]
            simple,
            #[strong]
            previews,
            #[strong]
            copy,
            #[strong]
            context,
            #[strong]
            rows,
            #[strong]
            owner,
            move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let root = row_frame();
                let eye = button("layer-eye-symbolic", copy.borrow().layer.show.as_ref());
                eye.add_css_class("layer-column");
                root.append(&eye);
                let selection = button(
                    "layer-selection-empty-symbolic",
                    copy.borrow().layer.select_row_help.as_ref(),
                );
                selection.add_css_class("layer-column");
                root.append(&selection);
                let thumbnails = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                let gutter = gtk::Box::new(gtk::Orientation::Vertical, 0);
                gutter.add_css_class("layer-connection-gutter");
                thumbnails.append(&gutter);
                let (content, content_image, content_preview) =
                    thumbnail(copy.borrow().layer.edit_content.as_ref());
                let effect_icon = gtk::Image::new();
                effect_icon.set_pixel_size(24);
                effect_icon.set_can_target(false);
                content_preview.add_overlay(&effect_icon);
                let pass_through = crate::icons::image("layer-group-pass-through-symbolic");
                pass_through.set_pixel_size(12);
                pass_through.set_halign(gtk::Align::End);
                pass_through.set_valign(gtk::Align::End);
                pass_through.set_margin_end(3);
                pass_through.set_margin_bottom(3);
                pass_through.set_can_target(false);
                pass_through.add_css_class("layer-type-symbol");
                pass_through.add_css_class("layer-group-pass-through");
                content_preview.add_overlay(&pass_through);
                thumbnails.append(&content);
                let link = button("layer-link-symbolic", copy.borrow().layer.link_mask_to_layer.as_ref());
                link.add_css_class("layer-link");
                thumbnails.append(&link);
                let (mask, mask_image, _) = thumbnail(copy.borrow().layer.edit_mask.as_ref());
                thumbnails.append(&mask);
                let texture_thumbnail = crate::paper_picker::PaperThumbnail::new(28);
                let texture = gtk::Button::builder().child(&texture_thumbnail.image).valign(gtk::Align::Center).visible(false).build();
                for class in ["flat", "layer-icon", "layer-thumbnail"] { texture.add_css_class(class); }
                texture.connect_clicked(glib::clone!(#[weak] item, #[strong] owner, move |_| {
                    let Some(row) = row_state(&item) else { return };
                    let Some(w) = owner.borrow().upgrade() else { return };
                    w.dispatch(row_button_action(&row, 2));
                    if row.paper.is_some_and(|paper| paper.editable) { crate::paper_picker::open(&w, row.id); }
                }));
                thumbnails.append(&texture);
                root.append(&thumbnails);
                let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
                text.set_hexpand(true);
                text.set_valign(gtk::Align::Center);
                text.set_margin_start(6);
                let name = row_name();
                let name_entry = gtk::Entry::builder()
                    .width_chars(1)
                    .max_width_chars(16)
                    .max_length(128)
                    .has_frame(false)
                    .build();
                let name_composition = crate::input::guard_entry_activation(&name_entry);
                name_entry.add_css_class("layer-name-entry");
                let name_stack = gtk::Stack::new();
                name_stack.set_hhomogeneous(false);
                name_stack.set_vhomogeneous(false);
                name_stack.add_named(&name, Some("name"));
                name_stack.add_named(&name_entry, Some("edit"));
                text.append(&name_stack);
                let meta = gtk::Label::new(None);
                meta.add_css_class("layer-meta");
                meta.add_css_class("dim-label");
                meta.set_xalign(0.);
                meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
                meta.set_width_chars(1);
                text.append(&meta);
                root.append(&text);
                let load_selection = button("layer-selection-load-symbolic", copy.borrow().load_selection.as_ref());
                load_selection.set_size_request(30, 30);
                load_selection.set_valign(gtk::Align::Center);
                thumbnails.insert_child_after(&load_selection, Some(&content));
                let lock = gtk::Image::new();
                lock.set_pixel_size(12);
                lock.set_size_request(12, -1);
                root.append(&lock);
                let grip = crate::icons::image("layer-grip-symbolic");
                grip.add_css_class("dim-label");
                grip.add_css_class("drag-immediate");
                root.append(&grip);
                for (b, kind) in [
                    (&eye, 0),
                    (&selection, 1),
                    (&content, 2),
                    (&link, 3),
                    (&mask, 4),
                    (&load_selection, 5),
                ] {
                    if let Some(w) = owner.borrow().upgrade() {
                        w.tooltips.bind(b, glib::clone!(
                            #[weak]
                            item,
                            #[strong]
                            owner,
                            #[upgrade_or]
                            None,
                            move |button| {
                                let row = row_state(&item)?;
                                let w = owner.borrow().upgrade()?;
                                let gpu = w.gpu.borrow();
                                let state = gpu.as_ref()?.session.state();
                                Some(state.settings.action_tooltip_localized(
                                    &button.tooltip_text().unwrap_or_default(),
                                    &row_button_action(&row, kind),
                                    state.platform,
                                    &w.localization(),
                                ))
                            }
                        ));
                    }
                    b.connect_clicked(glib::clone!(
                        #[weak]
                        item,
                        #[strong]
                        owner,
                        move |_| {
                            let Some(row) = row_state(&item) else { return };
                            let Some(w) = owner.borrow().upgrade() else {
                                return;
                            };
                            w.dispatch(row_button_action(&row, kind));
                            if let Some(fill) = row.fill_color.filter(|_| kind == 2) { crate::color_editor::edit_fill(&w, row.id, fill); }
                        }
                    ));
                }
                let range = gtk::GestureClick::new();
                range.set_button(1);
                range.set_propagation_phase(gtk::PropagationPhase::Capture);
                range.connect_released(glib::clone!(#[weak] item, #[strong] owner, move |gesture,_,_,_| {
                    if !gesture.current_event_state().contains(gdk::ModifierType::SHIFT_MASK) { return; }
                    let Some(row) = row_state(&item) else { return; };
                    let Some(w) = owner.borrow().upgrade() else { return; };
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    action(&w, A::SelectRow { id: row.id, extend: true, toggle: true });
                }));
                selection.add_controller(range);
                for (button,is_mask) in [(&content,false),(&mask,true)] {
                    let load = gtk::GestureClick::new();
                    load.set_button(1);
                    load.set_propagation_phase(gtk::PropagationPhase::Capture);
                    load.connect_pressed(glib::clone!(#[weak] item, #[strong] owner, move |gesture,_,_,_| {
                        let modifiers=gesture.current_event_state();
                        if !modifiers.contains(gdk::ModifierType::CONTROL_MASK) {return}
                        let Some(row)=row_state(&item) else {return};
                        if row.group {return}
                        let Some(w)=owner.borrow().upgrade() else {return};
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        w.dispatch(UiAction::Selection {action:layer_ui::SelectionAction::LoadThumbnail {
                            id:row.id, mask:is_mask, shift:modifiers.contains(gdk::ModifierType::SHIFT_MASK), alt:modifiers.contains(gdk::ModifierType::ALT_MASK),
                        }});
                    }));
                    button.add_controller(load);
                }
                let click = gtk::GestureClick::new();
                click.set_button(1);
                click.connect_released(glib::clone!(
                    #[weak]
                    item,
                    #[weak]
                    root,
                    #[strong]
                    owner,
                    move |gesture, _, x, y| {
                        // Only empty space/text selects. Buttons and the rename
                        // entry keep their own actions, including touch checks.
                        let picked = root.pick(x, y, gtk::PickFlags::DEFAULT);
                        let mut target = picked.clone();
                        while let Some(widget) = target {
                            if widget.is::<gtk::Button>() || widget.is::<gtk::Entry>() {
                                return;
                            }
                            if widget == root {
                                break;
                            }
                            target = widget.parent();
                        }
                        let Some(row) = row_state(&item) else { return };
                        let Some(w) = owner.borrow().upgrade() else {
                            return;
                        };
                        let modifiers = gesture.current_event_state();
                        action(&w, A::SelectRow {
                            id: row.id,
                            extend: modifiers.contains(gdk::ModifierType::SHIFT_MASK),
                            toggle: modifiers.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::META_MASK),
                        });
                    }
                ));
                root.add_controller(click);
                // The name owns double-click/tap recognition. A ListView's row
                // gesture can lose its click count when selection refreshes it.
                name.set_can_target(true);
                let rename = gtk::GestureClick::new();
                rename.set_button(1);
                rename.set_propagation_phase(gtk::PropagationPhase::Capture);
                rename.connect_pressed(glib::clone!(#[weak] item, #[strong] owner, move |gesture,n,_,_| {
                    if n != 2 { return; }
                    let Some(row) = row_state(&item).filter(|r| r.can_rename) else { return; };
                    let Some(w) = owner.borrow().upgrade() else { return; };
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    action(&w, A::BeginRename { id: row.id });
                }));
                name.add_controller(rename);

                name_entry.connect_activate(glib::clone!(
                    #[weak]
                    item,
                    #[weak]
                    name_stack,
                    #[strong]
                    owner,
                    move |entry| finish_name(&owner, &item, &name_stack, entry, false)
                ));
                let focus = gtk::EventControllerFocus::new();
                focus.connect_leave(glib::clone!(
                    #[weak]
                    item,
                    #[weak]
                    name_stack,
                    #[weak]
                    name_entry,
                    #[strong]
                    owner,
                    move |_| finish_name(&owner, &item, &name_stack, &name_entry, false)
                ));
                name_entry.add_controller(focus);
                let keys = gtk::EventControllerKey::new();
                keys.connect_key_pressed(glib::clone!(
                    #[strong]
                    name_composition,
                    #[weak]
                    item,
                    #[weak]
                    name_stack,
                    #[weak]
                    name_entry,
                    #[strong]
                    owner,
                    #[upgrade_or]
                    glib::Propagation::Proceed,
                    move |_, key, _, _| {
                        if key == gdk::Key::Escape {
                            if !name_composition.active() {
                                finish_name(&owner, &item, &name_stack, &name_entry, true);
                            }
                            glib::Propagation::Stop
                        } else {
                            glib::Propagation::Proceed
                        }
                    }
                ));
                name_entry.add_controller(keys);
                let drag = row_drag(item, &root, &name_stack, false, &owner, &context);
                root.add_controller(drag.source.clone());
                let grip_drag = row_drag(
                    item,
                    &root,
                    &name_stack,
                    true,
                    &owner,
                    &context,
                );
                grip.add_controller(grip_drag.source.clone());
                for (widget, is_mask) in [
                    (root.clone().upcast::<gtk::Widget>(), false),
                    (mask.clone().upcast(), true),
                ] {
                    let click = gtk::GestureClick::new();
                    click.set_button(3);
                    click.connect_pressed(glib::clone!(
                        #[strong]
                        context,
                        #[weak]
                        item,
                        #[strong]
                        owner,
                        move |g, _, x, y| {
                            let Some(row) = row_state(&item) else { return };
                            let Some(w) = owner.borrow().upgrade() else {
                                return;
                            };
                            g.set_state(gtk::EventSequenceState::Claimed);
                            context.set_autohide(true);
                            menu(&w, &context, &g.widget().unwrap(), row.id, is_mask, [x, y]);
                        }
                    ));
                    widget.add_controller(click);
                }
                let hold_menu = glib::clone!(
                    #[strong]
                    context,
                    #[weak]
                    item,
                    #[weak]
                    root,
                    #[weak]
                    mask,
                    #[weak]
                    name_stack,
                    #[strong]
                    owner,
                    move |g: &gtk::GestureLongPress, x: f64, y: f64| {
                        if !crate::input::touch_or_pen(g) {
                            return;
                        }
                        if name_stack.visible_child_name().as_deref() == Some("edit") {
                            return;
                        }
                        let Some(row) = row_state(&item) else {
                            return;
                        };
                        let Some(w) = owner.borrow().upgrade() else {
                            return;
                        };
                        let Some(point) = g.widget().and_then(|widget| widget.compute_point(&root,
                            &gtk::graphene::Point::new(x as f32, y as f32))) else { return; };
                        let is_mask = root
                            .pick(point.x() as f64, point.y() as f64, gtk::PickFlags::DEFAULT)
                            .is_some_and(|picked| picked == mask || picked.is_ancestor(&mask));
                        g.set_state(gtk::EventSequenceState::Claimed);
                        context.set_autohide(false);
                        menu(&w, &context, root.upcast_ref(), row.id, is_mask, [point.x() as f64, point.y() as f64]);
                    }
                );
                for widget in [root.clone().upcast::<gtk::Widget>(), grip.clone().upcast()] {
                    let hold = gtk::GestureLongPress::new();
                    hold.set_touch_only(false);
                    hold.set_propagation_phase(gtk::PropagationPhase::Capture);
                    hold.connect_pressed(hold_menu.clone());
                    widget.add_controller(hold.clone());
                    if widget == grip { hold.group_with(&grip_drag.source); }
                }
                let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
                drop.set_preload(true);
                drop.connect_accept(|_, drop| !drop.formats().contain_mime_type("text/uri-list"));
                let preview = glib::clone!(
                    #[weak] item, #[weak] root, #[weak] content, #[strong] owner, #[strong] rows,
                    #[upgrade_or] gdk::DragAction::empty(),
                    move |drop: &gtk::DropTarget, x: f64, y: f64| {
                        let hint = (|| {
                            let id = dragged_layer(&drop.value()?)?;
                            let row = row_state(&item)?;
                            let w = owner.borrow().upgrade()?;
                            let (fraction, surface) = drop_hit(&root, &content, x, y);
                            let gpu = w.gpu.borrow();
                            gpu.as_ref()?.session.layer_drop_preview(id, row.id, fraction, surface)
                        })();
                        show_drop_hint(&rows, hint);
                        if hint.is_some() { gdk::DragAction::MOVE } else { gdk::DragAction::empty() }
                    }
                );
                drop.connect_enter(preview.clone());
                drop.connect_motion(preview);
                drop.connect_leave(glib::clone!(#[strong] rows, move |_| show_drop_hint(&rows, None)));
                drop.connect_drop(glib::clone!(
                    #[weak] item, #[weak] root, #[weak] content, #[strong] owner, #[strong] rows,
                    #[upgrade_or] false,
                    move |_, value, x, y| {
                        show_drop_hint(&rows, None);
                        let Some(id) = dragged_layer(value) else { return false; };
                        let Some(row) = row_state(&item) else { return false; };
                        let Some(w) = owner.borrow().upgrade() else { return false; };
                        let (fraction, surface) = drop_hit(&root, &content, x, y);
                        let valid = w.gpu.borrow().as_ref().is_some_and(|g| g.session.layer_drop_preview(id, row.id, fraction, surface).is_some());
                        if valid { action(&w, A::Drop { id, target: row.id, fraction, surface }); }
                        valid
                    }
                ));
                root.add_controller(drop);
                crate::files::drop::install_row(&root, glib::clone!(
                    #[weak] item,
                    #[strong] owner,
                    #[upgrade_or] None,
                    move || Some((owner.borrow().upgrade()?, row_state(&item)?.id))
                ));
                let other_rows = Rc::downgrade(&rows);
                let swipe = crate::swipe_row::SwipeRow::new(&root,
                    glib::clone!(#[weak] item, #[strong] owner, move || {
                        if let (Some(row), Some(w)) = (row_state(&item), owner.borrow().upgrade())
                            && let Some(model) = row.swipe_actions.iter().find(|model|model.id == "delete") { action(&w, model.action.clone()); }
                    }),
                    glib::clone!(#[weak] item, #[strong] owner, move || {
                        if let (Some(row), Some(w)) = (row_state(&item), owner.borrow().upgrade())
                            && let Some(model) = row.swipe_actions.iter().find(|model|model.id == "lock") { action(&w, model.action.clone()); }
                    }),
                    glib::clone!(#[weak] item, #[strong] owner, move || {
                        if let (Some(row), Some(w)) = (row_state(&item), owner.borrow().upgrade())
                            && let Some(model) = row.swipe_actions.iter().find(|model|model.id == "duplicate") { action(&w, model.action.clone()); }
                    }),
                    glib::clone!(#[weak] item, #[strong] owner, move || {
                        if let (Some(row), Some(w)) = (row_state(&item), owner.borrow().upgrade())
                            && let Some(command) = row.right_swipe { action(&w, command); }
                    }),
                    move |opened| {
                        if let Some(rows) = other_rows.upgrade() {
                            for row in rows.borrow().values() {
                                if row.swipe != *opened && row.swipe.is_open() { row.swipe.reveal(false); }
                            }
                        }
                    }, glib::clone!(#[weak] connections, #[weak] grip, move |row| {
                        grip.set_opacity(1. - row.swipe_progress());
                        connections.queue_draw();
                    }));
                item.set_child(Some(&swipe));
                rows.borrow_mut().insert(
                    item.as_ptr() as usize,
                    Row {
                        simple: simple.clone(),
                        copy: copy.clone(),
                        owner: owner.clone(),
                        previews: previews.clone(),
                        id: Cell::new(0),
                        bound: Cell::new(false),
                        swipe,
                        content_image,
                        effect_icon,
                        pass_through,
                        mask_image,
                        root,
                        eye,
                        selection,
                        thumbnails,
                        content,
                        load_selection,
                        mask,
                        link,
                        texture,
                        texture_thumbnail,
                        name,
                        name_stack,
                        name_entry,
                        meta,
                        lock,
                        grip,
                        pickups: [drag, grip_drag],
                    },
                );
            }
        ));
        factory.connect_bind(glib::clone!(
            #[weak]
            connections,
            #[strong]
            owner,
            #[strong]
            rows,
            move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let rows = rows.borrow();
                let row = &rows[&(item.as_ptr() as usize)];
                if let Some(state) = row_state(item) {
                    if item.child().as_ref() != Some(row.swipe.upcast_ref()) { item.set_child(Some(&row.swipe)); }
                    item.set_accessible_label("");
                    item.set_accessible_description("");
                    row.bound.set(true);
                    row.refresh(&state);
                } else {
                    row.bound.set(false);
                    let object = item.item().unwrap().downcast::<glib::BoxedAnyObject>().unwrap();
                    let action = object.borrow::<layer_ui::PropertyActionView>();
                    let controls = paper_row(&action, owner.clone());
                    item.set_accessible_label(&action.label);
                    item.set_accessible_description("");
                    item.set_child(Some(&controls));
                }
                connections.queue_draw();
            }
        ));
        factory.connect_teardown(glib::clone!(
            #[strong]
            rows,
            move |_, item| {
                rows.borrow_mut().remove(&(item.as_ptr() as usize));
            }
        ));
        factory.connect_unbind(glib::clone!(
            #[strong]
            rows,
            move |_, item| {
                if let Some(row) = rows.borrow().get(&(item.as_ptr() as usize)) {
                    row.bound.set(false);
                    crate::files::drop::clear_row(&row.root);
                    // Selection can rebind the same row. Keep its image until
                    // refresh sees a different ID; detached rows are not polled.
                    row.swipe.reset();
                }
            }
        ));
        let selection = gtk::NoSelection::new(Some(model.clone()));
        let view = gtk::ListView::new(Some(selection), Some(factory));
        view.set_single_click_activate(false);
        view.connect_activate(glib::clone!(#[strong] model, #[strong] owner, move |_, position| {
            let Some(object) = model.item(position).and_downcast::<glib::BoxedAnyObject>() else { return };
            if object.try_borrow::<layer_ui::PropertyActionView>().is_err() { return; }
            activate_paper(&owner);
        }));
        view.add_css_class("layer-list");
        let list = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .min_content_height(0)
            .child(&view)
            .build());
        let overlay = gtk::Overlay::new();
        touch::install(&list, owner.clone(), rows.clone());
        overlay.set_child(Some(&list));
        let unit_row=row_frame();
        unit_row.append(&button("layer-eye-symbolic", ""));
        unit_row.append(&thumbnail("").0);
        unit_row.append(&row_name());
        unit_row.set_can_target(false);
        overlay.add_overlay(&unit_row);
        unit_row.set_child_visible(false);
        overlay.set_measure_overlay(&unit_row,false);
        overlay.add_overlay(&connections);
        overlay.set_measure_overlay(&connections, false);
        overlay.set_clip_overlay(&connections, true);
        list.vadjustment().connect_value_changed(glib::clone!(#[weak] connections, move |_| connections.queue_draw()));
        root.append(&overlay);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, layer_ui::PANEL_CONTROL_METRICS.compound_spacing as i32);
        footer.add_css_class("layer-footer");
        let footer_room = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(&footer)
            .build();
        let fitted = Cell::new(-1);
        footer_room.add_tick_callback(glib::clone!(#[weak] footer, #[upgrade_or] glib::ControlFlow::Break, move |room, _| {
            if fitted.replace(room.width()) != room.width() { fit_footer(&footer, room.width()); }
            glib::ControlFlow::Continue
        }));
        root.append(&footer_room);
        Self {
            copy: copy.clone(),
            root,
            header,
            footer,
            list,
            unit_row,
            connections,
            opacity,
            opacity_popup,
            opacity_editor,
            model,
            blend,
            blend_label,
            blend_menu,
            alpha,
            lock,
            mask_action: button(
                "layer-mask-symbolic",
                copy.borrow().layer.add_mask.as_ref(),
            ),
            reference,
            attachment,
            add_filter: {
                let button = gtk::MenuButton::new();
                button.set_child(Some(&crate::icons::image("layer-add-filter-symbolic")));
                button.add_css_class("flat");
                button.add_css_class("layer-icon"); caption(&button, copy.borrow().layer.add_filter.as_ref()); button
            },
            delete: button("layer-delete-symbolic", copy.borrow().layer.delete_selected.as_ref()),
            context,
            owner,
            rows,
            previews,
            requested: Default::default(),
            pending: Default::default(),
            simple,
            next_preview: Cell::new(1),
            renderer: Cell::new(0),
        }
    }
    pub fn content_measurement(&self, width: i32) -> (f32, layer_ui::PanelScrollMeasurement) {
        let measurement = self.scroll_measurement(width);
        ((measurement.fixed_height + measurement.unit_height * self.model.n_items() as f32).min(999_999.), measurement)
    }
    pub fn scroll_measurement(&self, width: i32) -> layer_ui::PanelScrollMeasurement {
        let fixed_height = (self.root.measure(gtk::Orientation::Vertical, width).1
            - self.list.measure(gtk::Orientation::Vertical, width).1)
            .max(0) as f32;
        let unit_height = self.unit_row.measure(gtk::Orientation::Vertical,width).1 as f32;
        layer_ui::PanelScrollMeasurement { fixed_height, unit_height }
    }
    pub fn reveal_row(&self, position: u32) {
        if let Some(list) = self.list.child().and_downcast::<gtk::ListView>() {
            list.scroll_to(position, gtk::ListScrollFlags::NONE, None);
        }
    }
    pub fn scroll_anchor(&self) -> Option<(u64, f32)> {
        self.rows.borrow().values().filter(|row| row.bound.get() && row.root.is_mapped()).filter_map(|row| {
            let bounds = row.root.compute_bounds(&self.list)?;
            (bounds.y() + bounds.height() > 0. && bounds.y() < self.list.height() as f32)
                .then_some((row.id.get(), bounds.y()))
        }).min_by(|a, b| a.1.total_cmp(&b.1))
    }
    pub fn restore_scroll_anchor(&self, (id, offset): (u64, f32)) -> bool {
        let position = self.rows.borrow().values().find(|row| row.bound.get() && row.root.is_mapped() && row.id.get() == id)
            .and_then(|row| row.root.compute_bounds(&self.list)).map(|bounds| bounds.y());
        if let Some(position) = position {
            let delta = position - offset;
            if delta.abs() < 0.5 { return true; }
            let adjustment = self.list.vadjustment();
            let before = adjustment.value();
            adjustment.set_value((before + delta as f64).clamp(adjustment.lower(), (adjustment.upper() - adjustment.page_size()).max(adjustment.lower())));
            return adjustment.value() == before;
        }
        for position in 0..self.model.n_items() {
            if self.model.item(position).and_downcast::<glib::BoxedAnyObject>().and_then(|object| item_id(&object)) == Some(id) {
                self.reveal_row(position);
                return false;
            }
        }
        true
    }

    pub fn bind(&self, w: &Rc<Workspace>) {
        let copy = self.copy.borrow();
        *self.owner.borrow_mut() = Rc::downgrade(w);
        w.watch_popover(self.context.upcast_ref());
        w.watch_popover(self.opacity_popup.upcast_ref());
        let surface = self.list.as_ptr() as usize as u64;
        self.opacity_popup.connect_closed(glib::clone!(#[weak] w, move |_| {
            let open = w.gpu.borrow().as_ref().is_some_and(|g| g.session.state().layer_tools.opacity_popup.is_some() && g.session.state().layer_tools.opacity_surface == surface);
            if open { w.dispatch(UiAction::Layer { action: A::CloseOpacity }); }
        }));
        self.opacity_editor.connect_value_changed(glib::clone!(#[weak] w, move |control| {
            let id = w.gpu.borrow().as_ref().and_then(|g| g.session.state().layer_tools.opacity_popup);
            if let Some(id) = id { w.dispatch(UiAction::SetLayerOpacity { id: Some(id), opacity: control.value() as f32 }); }
        }));
        if self.root == w.layer_panel.root {
            let click = gtk::GestureClick::new();
            click.set_button(0);
            click.set_propagation_phase(gtk::PropagationPhase::Capture);
            for release in [false, true] {
                let close = glib::clone!(#[weak] w, move |g: &gtk::GestureClick, _: i32, x: f64, y: f64| {
                    let Some(widget) = g.widget() else { return; };
                    let extra: Vec<_> = w.drawers().iter().filter_map(|d| d.layers()).chain(w.bubbles.layer_panels()).collect();
                    for panel in std::iter::once(&w.layer_panel).chain(extra.iter().map(|p| p.as_ref())) {
                        panel.close_swipes_at(&widget, [x, y], release);
                    }
                });
                if release { click.connect_released(close); } else { click.connect_pressed(close); }
            }
            w.window.add_controller(click);
            glib::timeout_add_local(
                std::time::Duration::from_millis(120),
                glib::clone!(
                    #[weak]
                    w,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        w.layer_panel.update_previews(
                            &w,
                            &w.drawers()
                                .iter()
                                .filter_map(|d| d.layers())
                                .chain(w.bubbles.layer_panels())
                                .collect::<Vec<_>>(),
                            &w.bubbles.thumbnails(),
                        );
                        glib::ControlFlow::Continue
                    }
                ),
            );
        }
        for (icon, label, a) in [
            ("layer-add-layer-symbolic", copy.layer.new_layer.as_ref(), UiAction::Layer { action: A::New { group: false, clipped: false } }),
            ("layer-add-vector-layer-symbolic", copy.layer.new_vector_layer.as_ref(), UiAction::Invoke { command: layer_ui::CommandId::NewVectorLayer }),
            ("layer-folder-symbolic", copy.layer.new_group.as_ref(), UiAction::Layer { action: A::New { group: true, clipped: false } }),
        ] {
            let b = button(icon, label);
            b.set_widget_name(icon);
            w.bind_action_tooltip(&b, a.clone());
            b.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| w.dispatch(a.clone())
            ));
            self.footer.append(&b);
        }
        let selection = button("layer-selection-brush-symbolic", copy.layer.new_selection_layer.as_ref());
        selection.set_widget_name("new-selection-layer");
        selection.connect_clicked(glib::clone!(#[weak] w, move |_| w.dispatch(UiAction::Invoke { command: layer_ui::CommandId::NewSelectionLayer })));
        self.footer.append(&selection);
        let mask = &self.mask_action;
        w.bind_dynamic_action_tooltip(mask, |s| {
            Some(UiAction::Layer {
                action: A::AddMask {
                    id: s.layer_tools.editing_layer.as_ref()?.id,
                    replace: false,
                },
            })
        });
        w.bind_action_tooltip(
            self.reference.upcast_ref(),
            UiAction::Layer {
                action: A::ReferenceSelection,
            },
        );
        for (button, kind) in [(&self.alpha, 0), (&self.lock, 1), (&self.attachment, 2)] {
            w.bind_dynamic_action_tooltip(button, move |s| {
                let row = s.layer_tools.editing_layer.as_ref()?;
                let id = row.id;
                Some(UiAction::Layer {
                    action: match kind {
                        0 => A::AlphaLock {
                            id,
                            value: !row.alpha_locked,
                        },
                        1 => A::Lock {
                            id,
                            value: !row.locked,
                        },
                        _ => s.layer_tools.attachment.action.clone()?,
                    },
                })
            });
        }
        mask.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                if let Some(id) = active(&w) {
                    action(&w, A::AddMask { id, replace: false });
                }
            }
        ));
        mask.set_widget_name("layer-add-mask");
        self.footer.append(mask);
        self.add_filter.set_widget_name("layer-add-filter");
        let filter_menu = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
        w.watch_popover(filter_menu.upcast_ref());
        self.add_filter.set_popover(Some(&filter_menu));
        filter_menu.connect_show(glib::clone!(#[weak] w, move |popover| {
            let menu = w.gpu.borrow().as_ref().and_then(|g| g.session.state().layer_tools.add_filter.clone());
            if let Some(menu) = menu { w.populate_workspace_menu(popover, menu); }
        }));
        self.footer.append(&self.add_filter);
        let import = button("layer-image-symbolic", copy.layer.import_image.as_ref());
        import.set_widget_name("import-image-layer");
        w.bind_action_tooltip(&import, UiAction::Invoke { command: layer_ui::CommandId::ImportImage });
        import.connect_clicked(glib::clone!(#[weak] w, move |_| {
            w.dispatch(UiAction::Invoke { command: layer_ui::CommandId::ImportImage });
        }));
        self.footer.append(&import);
        self.delete.set_widget_name("delete-selected-layers");
        w.bind_action_tooltip(
            &self.delete,
            UiAction::Layer {
                action: A::DeleteSelected,
            },
        );
        self.delete.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| action(&w, A::DeleteSelected)
        ));
        self.footer.append(&self.delete);
        let more = button("layer-more-small-symbolic", copy.layer.actions.as_ref());
        more.set_widget_name("layer-actions");
        more.set_hexpand(true);
        more.set_halign(gtk::Align::End);
        let context = &self.context;
        more.connect_clicked(glib::clone!(
            #[weak]
            context,
            #[weak]
            w,
            move |b| {
                let footer = b.parent().map_or(0, |footer| footer.parent().map_or(footer.width(), |room| room.width()));
                let overflow = w.gpu.borrow().as_ref().map(|g| layer_ui::layer_footer_overflow(&g.session.state().layer_tools.footer, footer as f32)).unwrap_or_default();
                let mask = active(&w).is_some_and(|id| w.gpu.borrow().as_ref().is_some_and(|g| {
                    g.session.state().layers.iter().any(|l| l.id == id && l.mask_selected)
                }));
                actions_menu(&w, &context, b.upcast_ref(), active(&w), mask, overflow);
            }
        ));
        self.footer.append(&more);
        self.opacity.connect_value_changed(glib::clone!(
            #[weak]
            w,
            move |v| w.dispatch(UiAction::SetLayerOpacity {
                id: None,
                opacity: v.value() as f32
            })
        ));
        w.watch_popover(self.blend_menu.upcast_ref());
        self.blend_menu.connect_show(glib::clone!(
            #[weak]
            w,
            move |popover| {
                let menu = active(&w).and_then(|id| w.gpu.borrow().as_ref()?.session.layer_blend_menu(id).ok());
                if let Some(menu) = menu {
                    w.populate_workspace_menu(popover, menu);
                }
            }
        ));
        self.alpha.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |b| {
                if let Some(id) = active(&w) {
                    action(
                        &w,
                        A::AlphaLock {
                            id,
                            value: b.is_active(),
                        },
                    );
                }
            }
        ));
        self.attachment.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let command = w.gpu.borrow().as_ref().and_then(|g| g.session.state().layer_tools.attachment.action.clone());
                if let Some(command) = command { action(&w, command); }
            }
        ));
        self.lock.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |b| {
                if let Some(id) = active(&w) {
                    action(
                        &w,
                        A::Lock {
                            id,
                            value: b.is_active(),
                        },
                    );
                }
            }
        ));
        self.reference.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| action(&w, A::ReferenceSelection)
        ));
    }

    fn close_swipes_at(&self, widget: &gtk::Widget, point: [f64; 2], release: bool) {
        for row in self.rows.borrow().values().filter(|r| r.swipe.is_open()) {
            let local = widget.compute_point(&row.swipe, &gtk::graphene::Point::new(point[0] as f32, point[1] as f32));
            let picked = local.and_then(|p| row.swipe.pick(p.x() as f64, p.y() as f64, gtk::PickFlags::DEFAULT));
            let on_action = picked.as_ref().is_some_and(|picked| row.swipe.is_action(picked));
            if picked.is_none() || (release && !on_action) { row.swipe.reveal(false); }
        }
    }
    pub(crate) fn refresh_theme(&self) { self.connections.queue_draw(); }
    pub(crate) fn set_localization(&self, localization: std::sync::Arc<layer_ui::Localizer>) {
        *self.copy.borrow_mut() = LayerCopy::new(&localization);
        let copy = self.copy.borrow();
        caption(&self.blend, &copy.layer.blend);
        caption(&self.alpha, &copy.layer.alpha_lock);
        caption(&self.lock, &copy.layer.lock_editing);
        caption(&self.reference, &copy.reference);
        caption(&self.mask_action, &copy.layer.add_mask);
        caption(&self.add_filter, &copy.layer.add_filter);
        caption(&self.delete, &copy.layer.delete_selected);
        self.opacity.set_caption(&copy.layer.opacity, "", localization);
        for child in self.footer.observe_children().iter::<glib::Object>().flatten().filter_map(|object| object.downcast::<gtk::Widget>().ok()) {
            let label = match child.widget_name().as_str() {
                "new-selection-layer" => Some(&copy.layer.new_selection_layer),
                "layer-add-layer-symbolic" => Some(&copy.layer.new_layer),
                "layer-add-vector-layer-symbolic" => Some(&copy.layer.new_vector_layer),
                "layer-folder-symbolic" => Some(&copy.layer.new_group),
                "import-image-layer" => Some(&copy.layer.import_image),
                "layer-actions" => Some(&copy.layer.actions),
                _ => None,
            };
            if let Some(label) = label { caption(&child, label); }
        }
        for i in 0..self.model.n_items() {
            let Some(object) = self.model.item(i).and_downcast::<glib::BoxedAnyObject>() else { continue };
            if let Ok(state) = object.try_borrow::<LayerState>() {
                for row in self.rows.borrow().values().filter(|row| row.id.get() == state.id) { row.refresh(&state); }

            }
        }
    }
    pub fn set_simple(&self, simple: bool) {
        if self.simple.replace(simple) == simple {
            return;
        }
        self.header.set_visible(!simple);
        let inset = if simple { layer_ui::PANEL_CONTENT_INSET } else { layer_ui::PANEL_CONTROL_METRICS.compound_spacing } as i32;
        self.root.set_margin_top(inset);
        self.root.set_margin_bottom(inset);
        for row in self.rows.borrow().values() {
            row.meta.set_visible(!simple && !row.meta.text().is_empty());
        }
    }
    pub fn refresh(&self, state: &UiState) {
        let add_paper = state.layer_tools.add_paper.is_some();
        let ids: Vec<_> = state.layers.iter().map(|layer| Some(layer.id)).chain(add_paper.then_some(None)).collect();
        let same = self.model.n_items() as usize == ids.len()
            && ids.iter().enumerate().all(|(i, id)| {
                self.model.item(i as u32).and_downcast::<glib::BoxedAnyObject>().and_then(|o| item_id(&o)) == *id
            })
            && state.layer_tools.add_paper.as_ref().is_none_or(|action| {
                self.model.item(state.layers.len() as u32).and_downcast::<glib::BoxedAnyObject>()
                    .is_some_and(|object| object.try_borrow::<layer_ui::PropertyActionView>().is_ok_and(|previous| *previous == *action))
            });
        if !same {
            self.model.splice(0, self.model.n_items(), &panel_items(&state.layers, state.layer_tools.add_paper.as_ref()));
        } else {
            for (position, l) in state.layers.iter().enumerate() {
                let object = self.model.item(position as u32).and_downcast::<glib::BoxedAnyObject>().unwrap();
                if *object.borrow::<LayerState>() != *l {
                    *object.borrow_mut::<LayerState>() = l.clone();
                    for row in self.rows.borrow().values().filter(|row| row.id.get() == l.id) { row.refresh(l); }
                }

            }
        }
        if let Some(l) = &state.layer_tools.editing_layer {
            self.opacity.set_value(l.opacity as f64);
            if state.layer_tools.opacity_popup == Some(l.id) && state.layer_tools.opacity_surface == self.list.as_ptr() as usize as u64 && self.root.is_mapped() {
                self.opacity_editor.set_value(l.opacity as f64);
                if !self.opacity_popup.is_visible() { self.opacity_popup.popup(); }
            } else { self.opacity_popup.popdown(); }
            self.blend_label.set_text(&l.blend_label);
            self.alpha.set_active(l.alpha_locked);
            self.lock.set_active(l.locked);
        } else { self.opacity_popup.popdown(); }
        let attachment = &state.layer_tools.attachment;
        self.attachment.set_active(attachment.checked);
        self.attachment.set_sensitive(attachment.action.is_some());
        crate::icons::set_button(&self.attachment, attachment.icon);
        caption(&self.attachment, &attachment.label);
        self.attachment.set_tooltip_text(Some(&attachment.description));
        self.attachment.update_property(&[gtk::accessible::Property::Description(&attachment.description)]);
        self.connections.refresh(&state.layers, &state.layer_tools.connections);
        let controls = state.layer_tools.controls;
        self.opacity.set_sensitive(controls.opacity);
        self.blend.set_sensitive(controls.blend);
        self.alpha.set_sensitive(controls.alpha_lock);
        self.lock.set_sensitive(controls.edit_lock);
        self.mask_action.set_sensitive(controls.mask);
        self.add_filter.set_sensitive(state.layer_tools.add_filter.is_some());
        self.delete.set_sensitive(state.layer_tools.can_delete);
        self.reference
            .set_active(state.layer_tools.references_selected);
        self.reference
            .set_sensitive(state.layer_tools.can_reference);
        caption(&self.reference, self.copy.borrow().reference.as_ref());
        let rename = state.layer_tools.rename_layer.and_then(|id| {
            self.rows
                .borrow()
                .values()
                .find(|row| row.id.get() == id)
                .cloned()
        });
        if let Some(row) = rename
            && row.name_stack.visible_child_name().as_deref() != Some("edit")
        {
            row.name_entry.set_text(&row.name.text());
            row.name_stack.set_visible_child_name("edit");
            row.name_entry.grab_focus();
            row.name_entry.select_region(0, -1);
        }
    }
    pub(crate) fn preview(&self, id: u64) -> Option<gdk::Texture> {
        self.previews.borrow().get(&(id, false)).map(|(_, texture)| texture.clone())
    }
    fn update_previews(&self, w: &Workspace, extra: &[Rc<Self>], thumbnails: &[(u64, gtk::Picture)]) {
        let views: Vec<&Self> = std::iter::once(self).chain(extra.iter().map(|v| v.as_ref())).filter(|v| v.root.is_mapped()).collect();
        let shown = |view: &Self, widget: &gtk::Box| widget.compute_bounds(&view.list)
            .is_some_and(|b| b.y() + b.height() > 0. && b.y() < view.list.height() as f32);
        let rows: Vec<_> = views.iter().flat_map(|view| view.rows.borrow().values()
            .filter(|r| r.bound.get() && shown(view, &r.root)).cloned().collect::<Vec<_>>()).collect();
        let thumbnails: Vec<_> = thumbnails.iter().filter(|(_, picture)| picture.is_mapped()).collect();
        if rows.is_empty() && thumbnails.is_empty() {
            return;
        }
        let mut gpu = w.gpu.borrow_mut();
        let Some(g) = gpu.as_mut() else { return };
        if g.session.renderer_mut().ready().is_err() {
            return;
        }
        let generation = g.session.renderer_generation();
        if self.renderer.replace(generation) != generation {
            let lost: Vec<_> = self.pending.borrow_mut().drain().map(|(_, request)| request).collect();
            let mut requested = self.requested.borrow_mut();
            for (id, mask, revision) in lost {
                if requested.get(&(id, mask)) == Some(&revision) { requested.remove(&(id, mask)); }
            }
        }
        while let Some(request) = g.session.renderer_mut().take_cancelled_thumbnail() {
            let pending = self.pending.borrow_mut().remove(&request);
            if let Some((id, mask, revision)) = pending {
                let current = self.requested.borrow().get(&(id, mask)) == Some(&revision);
                if current { self.requested.borrow_mut().remove(&(id, mask)); }
            }
        }
        while let Some(result) = g.session.renderer_mut().take_thumbnail() {
            if let Ok(image) = result
                && let Some((id, mask, revision)) =
                    self.pending.borrow_mut().remove(&image.request_id)
                && self.requested.borrow().get(&(id, mask)) == Some(&revision)
            {
                let texture = g.session.engine().backend().view_color.texture(
                    [image.width, image.height], gdk::MemoryFormat::R8g8b8a8,
                    image.stride as usize, image.bytes);
                self.previews
                    .borrow_mut()
                    .insert((id, mask), (revision, texture));
            }
        }
        if g.session.engine().has_pending_document_edits() {
            return;
        }
        let mut wanted = Vec::new();
        for row in rows {
            let Some(state) = g.session.state().layers.iter().find(|l| l.id == row.id.get()).cloned() else { continue };
            if state.group {
                continue;
            }
            wanted.push((state.id, false, state.has_thumbnail.then_some(state.id), state.paint_revision, row.content_image.clone()));
            wanted.push((state.id, true, state.mask_id, state.mask_revision, row.mask_image.clone()));
        }
        for (id, picture) in thumbnails {
            let Some(state) = g.session.state().layers.iter().find(|l| l.id == *id && !l.group) else { continue };
            wanted.push((state.id, false, state.has_thumbnail.then_some(state.id), state.paint_revision, picture.clone()));
        }
        for (id, mask, target, revision, picture) in wanted {
            let Some(target) = target else { continue };
            let key = (id, mask);
            if let Some((_, texture)) = self.previews.borrow().get(&key) {
                picture.set_paintable(Some(texture));
            }
            if self.requested.borrow().get(&key) == Some(&revision)
                || self.pending.borrow().len() >= 8
            {
                continue;
            }
            let request = self.next_preview.get();
            self.next_preview.set(request + 1);
            if g.session
                .renderer_mut()
                .request_thumbnail(request, layer_render::ThumbnailTarget::from_wire_id(target).expect("Published thumbnail target"))
                .is_ok()
            {
                self.requested.borrow_mut().insert(key, revision);
                self.pending
                    .borrow_mut()
                    .insert(request, (id, mask, revision));
            }
        }
    }
}
fn footer_button(name: &str) -> Option<layer_ui::LayerFooterButton> {
    use layer_ui::LayerFooterButton as B;
    Some(match name {
        "layer-add-layer-symbolic" => B::NewLayer,
        "layer-add-vector-layer-symbolic" => B::NewVectorLayer,
        "layer-folder-symbolic" => B::NewGroup,
        "new-selection-layer" => B::NewSelectionLayer,
        "layer-add-mask" => B::AddMask,
        "layer-add-filter" => B::AddFilter,
        "import-image-layer" => B::ImportImage,
        "delete-selected-layers" => B::Delete,
        _ => return None,
    })
}
fn fit_footer(footer: &gtk::Box, width: i32) {
    let mut child = footer.first_child();
    while let Some(widget) = child {
        if let Some(button) = footer_button(widget.widget_name().as_str()) {
            widget.set_visible(width as f32 + 0.5 >= button.min_width());
        }
        child = widget.next_sibling();
    }
}
fn actions_menu(
    w: &Rc<Workspace>,
    context: &gtk::PopoverMenu,
    anchor: &gtk::Widget,
    id: Option<u64>,
    mask: bool,
    overflow: Vec<layer_ui::ContextMenuItem>,
) {
    if let Some(id) = id { action(w, A::Context { id, mask }); }
    let menu = id.and_then(|id| w.gpu.borrow().as_ref().and_then(|g| g.session.layer_menu(id, mask).ok()));
    if menu.is_none() && overflow.is_empty() { return; }
    let mut menu = menu.unwrap_or_else(|| layer_ui::ContextMenu { title: w.localization().text(layer_ui::MessageId::WORKSPACE_CONTROL_LAYER_ACTIONS).to_string(), sections: Vec::new() });
    if !overflow.is_empty() { menu.sections.insert(0, overflow); }
    show_menu(w, context, anchor, menu, [0., anchor.height() as f64]);
}
fn menu(
    w: &Rc<Workspace>,
    context: &gtk::PopoverMenu,
    anchor: &gtk::Widget,
    id: u64,
    mask: bool,
    at: [f64; 2],
) {
    action(w, A::Context { id, mask });
    let menu = w
        .gpu
        .borrow()
        .as_ref()
        .and_then(|g| g.session.layer_menu(id, mask).ok());
    let Some(menu) = menu else { return };
    show_menu(w, context, anchor, menu, at);
}
fn show_menu(w: &Rc<Workspace>, context: &gtk::PopoverMenu, anchor: &gtk::Widget, menu: layer_ui::ContextMenu, [x, y]: [f64; 2]) {
    w.populate_workspace_menu(context, menu);
    if let Some(p) = context.parent().and_then(|root| {
        anchor.compute_point(&root, &gtk::graphene::Point::new(x as f32, y as f32))
    }) {
        context.set_pointing_to(Some(&gdk::Rectangle::new(p.x() as i32, p.y() as i32, 1, 1)));
    }
    context.popup();
}
impl Drop for LayerPanel {
    fn drop(&mut self) {
        self.context.unparent();
    }
}
fn active(w: &Workspace) -> Option<u64> {
    w.gpu
        .borrow()
        .as_ref()?
        .session
        .state()
        .layer_tools
        .editing_layer
        .as_ref()
        .map(|l| l.id)
}
impl Row {
    fn refresh(&self, s: &LayerState) {
        let copy = self.copy.borrow();
        if self.id.replace(s.id) != s.id {
            self.swipe.reset();
            self.name_stack.set_visible_child_name("name");
            self.name_entry.set_text("");
            let previews = self.previews.borrow();
            self.content_image.set_paintable(previews.get(&(s.id, false)).map(|(_, texture)| texture));
            self.mask_image.set_paintable(previews.get(&(s.id, true)).map(|(_, texture)| texture));
        }
        self.root.set_widget_name(&format!("art-layer-{}", s.id));
        self.swipe.set_actions(s);
        let thumbnail = s.has_thumbnail && !s.adjustment_effect;
        self.effect_icon.set_visible(s.group || s.content_icon.is_some() && !s.selection_layer);
        self.content_image.set_visible(thumbnail);
        self.effect_icon.set_pixel_size(if thumbnail { 12 } else { 24 });
        self.effect_icon.set_halign(if thumbnail { gtk::Align::End } else { gtk::Align::Center });
        self.effect_icon.set_valign(if thumbnail { gtk::Align::End } else { gtk::Align::Center });
        self.effect_icon.set_margin_end(if thumbnail { 3 } else { 0 });
        self.effect_icon.set_margin_bottom(if thumbnail { 3 } else { 0 });
        if thumbnail && self.effect_icon.is_visible() { self.effect_icon.add_css_class("layer-type-symbol"); }
        else { self.effect_icon.remove_css_class("layer-type-symbol"); }
        if s.adjustment_effect { self.content.add_css_class("layer-effect"); }
        else { self.content.remove_css_class("layer-effect"); }
        crate::icons::set(&self.effect_icon, if s.group { Some(if s.collapsed { "layer-folder-symbolic" } else { "layer-folder-open-symbolic" }) } else { s.content_icon.as_deref() });
        let simple = self.simple.get();
        self.pass_through.set_visible(s.pass_through);
        self.selection.set_visible(!simple);
        self.load_selection.set_visible(s.selection_layer && !simple);
        caption(&self.load_selection, copy.load_selection.as_ref());
        self.load_selection.set_widget_name(&format!("selection-load-{}", s.id));
        self.name.set_text(&s.label);
        self.name.set_tooltip_text(Some(&s.label));
        self.thumbnails
            .set_margin_start((s.depth * 8).min(24) as i32);
        if s.selected {
            self.root.add_css_class("selected");
        } else {
            self.root.remove_css_class("selected");
        }
        for (button, selected) in [(&self.content, s.content_selected), (&self.mask, s.mask_selected)] {
            if selected { button.add_css_class("layer-editing"); }
            else { button.remove_css_class("layer-editing"); }
        }
        let drawing_target = s.drawing;
        crate::icons::set_button(&self.selection, s.selection_icon);
        caption(&self.selection, copy.layer.select_row_help.as_ref());
        self.selection.update_property(&[gtk::accessible::Property::Description(if s.reference { copy.reference.as_ref() } else { "" })]);
        self.selection.update_state(&[gtk::accessible::State::Selected(Some(s.selected))]);
        if let Some(icon) = self.selection.child() {
            icon.update_property(&[gtk::accessible::Property::Label(if drawing_target { copy.layer.drawing_target.as_ref() } else { "" })]);
        }
        crate::icons::set_button(
            &self.eye,
            if s.visible && !s.visibility_blocked {
                "layer-eye-symbolic"
            } else {
                "layer-eye-hidden-symbolic"
            },
        );
        self.eye.set_opacity(if s.visibility_blocked { 0.35 } else { 1. });
        caption(&self.eye, match (s.selection_layer,s.visible) {
            (true,true) => copy.layer.hide_selection.as_ref(), (true,false) => copy.layer.show_selection.as_ref(),
            (false,true) => copy.layer.hide.as_ref(), (false,false) => copy.layer.show.as_ref(),
        });
        self.link.set_visible(s.has_mask && !simple);
        self.mask.set_visible(s.has_mask && !simple);
        self.texture.set_visible(s.paper.is_some() && !simple);
        self.texture.set_widget_name(&format!("layer-texture-{}", s.id));
        if let Some(paper) = &s.paper {
            caption(&self.texture, copy.choose_texture.as_ref());
            self.texture.update_property(&[gtk::accessible::Property::Description(&paper.label)]);
            if let Some(w) = self.owner.borrow().upgrade() { self.texture_thumbnail.show(&w, paper.id.as_ref()); }
        }
        crate::icons::set_button(&self.link, if s.mask_linked { "layer-link-symbolic" } else { "layer-unlink-symbolic" });
        self.link.set_sensitive(!s.locked);
        caption(&self.link, if s.mask_linked { copy.layer.unlink_mask.as_ref() } else { copy.layer.link_mask_to_layer.as_ref() });
        self.mask_image
            .set_opacity(if s.mask_enabled { 1. } else { 0.4 });
        self.grip.set_visible(s.can_drop_below && !simple);
        for pickup in &self.pickups { pickup.retire_if_ineligible(s); }
        if s.group {
            self.content.add_css_class("layer-folder");
            caption(&self.content, if s.collapsed { copy.layer.expand.as_ref() } else { copy.layer.collapse.as_ref() });
        } else {
            self.content.remove_css_class("layer-folder");
            caption(&self.content, copy.layer.edit_content.as_ref());
        }
        crate::icons::set(
            &self.lock,
            Some(if s.locked {
                "layer-lock-symbolic"
            } else {
                "layer-alpha-lock-symbolic"
            }),
        );
        self.lock
            .set_opacity(if s.locked || s.alpha_locked { 1. } else { 0. });
        self.lock.update_state(&[gtk::accessible::State::Hidden(!s.locked && !s.alpha_locked)]);
        caption(&self.lock, if s.locked { copy.layer.locked.as_ref() } else { copy.layer.alpha_locked.as_ref() });
        self.meta.set_text(&s.description);
        self.meta.set_visible(!self.simple.get() && !s.description.is_empty());
    }
}

#[cfg(test)]
mod copy_tests {
    use super::*;

    #[test]
    #[ignore = "private display native_layers_control_copy"]
    fn native_layers_control_copy() {
        adw::init().unwrap();
        for theme in [adw::ColorScheme::ForceLight, adw::ColorScheme::ForceDark] {
            adw::StyleManager::default().set_color_scheme(theme);
            for language in layer_ui::UiLanguage::ALL {
                let context = layer_ui::Localizer::shared(language);
                let expected = layer_ui::NativeCopy::new(&context).layers;
                let panel = LayerPanel::new(context.clone());
                assert_eq!(panel.blend.tooltip_text().as_deref(), Some(expected.blend.as_ref()));
                assert_eq!(panel.blend_label.text().as_str(), context.text(layer_ui::MessageId::RESOURCES_BLEND_NORMAL).as_ref());
                assert_eq!(panel.alpha.tooltip_text().as_deref(), Some(expected.alpha_lock.as_ref()));
                assert_eq!(panel.lock.tooltip_text().as_deref(), Some(expected.lock_editing.as_ref()));
                assert_eq!(panel.attachment.tooltip_text().as_deref(), Some(context.text(layer_ui::MessageId::RESOURCES_LAYER_ATTACH_UNAVAILABLE).as_ref()));
                assert_eq!(panel.delete.tooltip_text().as_deref(), Some(expected.delete_selected.as_ref()));
                assert_eq!(panel.mask_action.tooltip_text().as_deref(), Some(expected.add_mask.as_ref()));
                assert!(std::sync::Arc::ptr_eq(&panel.copy.borrow().layer.blend, &expected.blend));
                assert!(gtk::test_accessible_has_property(&panel.blend, gtk::AccessibleProperty::Label));
            }
        }
    }
}
