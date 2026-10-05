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

struct LayerCopy {
    layer: layer_ui::native_copy::LayerCopy,
    reference: std::sync::Arc<str>,
    load_selection: std::sync::Arc<str>,
}
impl LayerCopy {
    fn new(localization: &layer_ui::Localizer) -> Self {
        Self {
            layer: layer_ui::NativeCopy::new(localization).layers,
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
    model: gio::ListStore,
    blend: gtk::MenuButton,
    blend_label: gtk::Label,
    blend_menu: gtk::PopoverMenu,
    alpha: gtk::ToggleButton,
    lock: gtk::ToggleButton,
    mask_action: gtk::Button,
    add_filter: gtk::MenuButton,
    color_mode: gtk::MenuButton,
    delete: gtk::Button,
    reference: gtk::ToggleButton,
    attachment: gtk::ToggleButton,
    context: gtk::PopoverMenu,
    owner: Rc<RefCell<Weak<Workspace>>>,
    rows: Rc<RefCell<HashMap<usize, Row>>>,
    connections: connections::Connections,
    previews: RefCell<HashMap<(u64, bool), (u64, gdk::Texture)>>,
    requested: RefCell<HashMap<(u64, bool), u64>>,
    pending: RefCell<HashMap<u64, (u64, bool, u64)>>,
    next_preview: Cell<u64>,
}
#[derive(Clone)]
struct Row {
    copy: Rc<RefCell<LayerCopy>>,
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
    content_frame: gtk::DrawingArea,
    mask: gtk::Button,
    mask_frame: gtk::DrawingArea,
    link: gtk::Button,
    name: gtk::Label,
    name_stack: gtk::Stack,
    name_entry: gtk::Entry,
    meta: gtk::Label,
    lock: gtk::Image,
    grip: gtk::Image,
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
    Some(
        item.item()?
            .downcast::<glib::BoxedAnyObject>()
            .ok()?
            .borrow::<LayerState>()
            .clone(),
    )
}
fn thumbnail(tooltip: &str) -> (gtk::Button, gtk::Picture, gtk::Overlay, gtk::DrawingArea) {
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
    // Above the thumbnail, so opaque image pixels cannot obscure the marks.
    let frame = gtk::DrawingArea::new();
    frame.set_can_target(false);
    frame.set_draw_func(|_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let bounds = gtk::graphene::Rect::new(1.5, 1.5, (w - 3.) as f32, (h - 3.) as f32);
        crate::squircle::rounded_rect(cr, &gtk::gsk::RoundedRect::from_rect(bounds, (w.min(h) - 3.) as f32 / 2.));
        cr.set_source_rgba(0., 0., 0., 0.8);
        cr.set_line_width(3.);
        let _ = cr.stroke_preserve();
        cr.set_source_rgb(1., 1., 1.);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    });
    overlay.add_overlay(&frame);
    button.set_child(Some(&overlay));
    (button, picture, overlay, frame)
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
    held: &Rc<Cell<bool>>,
    owner: &Rc<RefCell<Weak<Workspace>>>,
    context: &gtk::PopoverMenu,
) -> gtk::DragSource {
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    source.set_propagation_phase(gtk::PropagationPhase::Capture);
    source.connect_prepare(glib::clone!(
        #[strong]
        held,
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
            if name.visible_child_name().as_deref() == Some("edit")
                || (!handle
                    && !held.get()
                    && (source
                        .current_event()
                        .is_some_and(|e| e.device_tool().is_some())
                        || source.current_event_device().is_some_and(|d| {
                            matches!(
                                d.source(),
                                gdk::InputSource::Touchscreen | gdk::InputSource::Pen
                            )
                        })))
            {
                return None;
            }
            let row = row_state(&item)?;
            if !row.can_drop_below {
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
            Some(gdk::ContentProvider::for_value(
                &format!("capy-layer:{}", row.id).to_value(),
            ))
        }
    ));
    source
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
    let scale = row.native().and_then(|native| native.surface()).map_or(f64::from(row.scale_factor()), |surface| surface.scale());
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
        let copy = Rc::new(RefCell::new(LayerCopy::new(&localization)));
        let owner: Rc<RefCell<Weak<Workspace>>> = Rc::default();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("layers-panel");
        let context = gtk::PopoverMenu::from_model(None::<&gio::Menu>);
        context.set_parent(&root);
        context.set_has_arrow(false);
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
        let color_mode = gtk::MenuButton::builder().label("").hexpand(true).build();
        color_mode.set_widget_name("layer-color-mode");
        color_mode.add_css_class("layer-blend"); header.append(&color_mode);
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
        let connections = connections::Connections::new(rows.clone());
        factory.connect_setup(glib::clone!(
            #[weak]
            connections,
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
                let root = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                root.add_css_class("layer-row");
                root.add_css_class("customizable-target");
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
                let (content, content_image, content_preview, content_frame) =
                    thumbnail(copy.borrow().layer.edit_content.as_ref());
                let effect_icon = gtk::Image::new();
                effect_icon.set_pixel_size(24);
                effect_icon.set_can_target(false);
                content_preview.remove_overlay(&content_frame);
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
                content_preview.add_overlay(&content_frame);
                thumbnails.append(&content);
                let link = button("layer-link-symbolic", copy.borrow().layer.link_mask_to_layer.as_ref());
                link.add_css_class("layer-link");
                thumbnails.append(&link);
                let (mask, mask_image, _, mask_frame) = thumbnail(copy.borrow().layer.edit_mask.as_ref());
                thumbnails.append(&mask);
                root.append(&thumbnails);
                let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
                text.set_hexpand(true);
                text.set_valign(gtk::Align::Center);
                text.set_margin_start(6);
                let name = gtk::Label::new(None);
                name.add_css_class("layer-name");
                name.set_xalign(0.);
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                name.set_width_chars(1);
                name.set_hexpand(true);
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
                let held = Rc::new(Cell::new(false));
                let drag = row_drag(item, &root, &name_stack, false, &held, &owner, &context);
                root.add_controller(drag.clone());
                grip.add_controller(row_drag(
                    item,
                    &root,
                    &name_stack,
                    true,
                    &held,
                    &owner,
                    &context,
                ));
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
                let hold = gtk::GestureLongPress::new();
                hold.set_touch_only(false); // Pen events are not touch sequences.
                hold.set_propagation_phase(gtk::PropagationPhase::Capture);
                hold.connect_begin(glib::clone!(
                    #[strong]
                    held,
                    move |_, _| held.set(false)
                ));
                hold.connect_pressed(glib::clone!(
                    #[strong]
                    context,
                    #[strong]
                    held,
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
                    move |g, x, y| {
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
                        let is_mask = root
                            .pick(x, y, gtk::PickFlags::DEFAULT)
                            .is_some_and(|picked| picked == mask || picked.is_ancestor(&mask));
                        held.set(row.can_drop_below);
                        // Grouping preserves the row DragSource while claiming
                        // the held contact from scrolling and child buttons.
                        g.set_state(gtk::EventSequenceState::Claimed);
                        context.set_autohide(false);
                        menu(&w, &context, root.upcast_ref(), row.id, is_mask, [x, y]);
                    }
                ));
                root.add_controller(hold.clone());
                hold.group_with(&drag);
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
                        if let (Some(row), Some(w)) = (row_state(&item), owner.borrow().upgrade()) {
                            action(&w, A::Delete { id: row.id });
                        }
                    }),
                    glib::clone!(#[weak] item, #[strong] owner, move || {
                        if let (Some(row), Some(w)) = (row_state(&item), owner.borrow().upgrade()) {
                            if let Some(command) = row.right_swipe { action(&w, command); }
                        }
                    }),
                    move |opened| {
                        if let Some(rows) = other_rows.upgrade() {
                            for row in rows.borrow().values() {
                                if row.swipe != *opened && row.swipe.is_open() { row.swipe.reveal(false); }
                            }
                        }
                    }, glib::clone!(#[weak] connections, move || connections.queue_draw()));
                item.set_child(Some(&swipe));
                rows.borrow_mut().insert(
                    item.as_ptr() as usize,
                    Row {
                        copy: copy.clone(),
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
                        content_frame,
                        mask,
                        mask_frame,
                        link,
                        name,
                        name_stack,
                        name_entry,
                        meta,
                        lock,
                        grip,
                    },
                );
            }
        ));
        factory.connect_bind(glib::clone!(
            #[weak]
            connections,
            #[strong]
            rows,
            move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let Some(state) = row_state(item) else { return };
                let rows = rows.borrow();
                let row = &rows[&(item.as_ptr() as usize)];
                row.bound.set(true);
                row.refresh(&state);
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
        view.add_css_class("layer-list");
        let list = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .min_content_height(0)
            .child(&view)
            .build());
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&list));
        overlay.add_overlay(&connections);
        overlay.set_measure_overlay(&connections, false);
        overlay.set_clip_overlay(&connections, true);
        list.vadjustment().connect_value_changed(glib::clone!(#[weak] connections, move |_| connections.queue_draw()));
        root.append(&overlay);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        footer.add_css_class("layer-footer");
        root.append(&footer);
        Self {
            copy: copy.clone(),
            root,
            header,
            footer,
            list,
            connections,
            opacity,
            model,
            blend,
            blend_label,
            blend_menu,
            color_mode,
            alpha,
            lock,
            mask_action: button(
                "layer-mask-symbolic",
                copy.borrow().layer.add_mask.as_ref(),
            ),
            reference,
            attachment,
            add_filter: {
                let button = gtk::MenuButton::builder().icon_name("layer-adjustments-symbolic").build();
                button.add_css_class("layer-icon"); caption(&button, copy.borrow().layer.add_filter.as_ref()); button
            },
            delete: button("layer-delete-symbolic", copy.borrow().layer.delete_selected.as_ref()),
            context,
            owner,
            rows,
            previews: Default::default(),
            requested: Default::default(),
            pending: Default::default(),
            next_preview: Cell::new(1),
        }
    }
    /// The virtual ListView's request describes its viewport, not all rows.
    /// Layer rows have a uniform height; sample realized rows and multiply by
    /// the visible model count, without instantiating a large document's rows.
    pub fn content_measurement(&self, width: i32) -> (f32, layer_ui::PanelScrollMeasurement) {
        let fixed_height = (self.root.measure(gtk::Orientation::Vertical, width).1
            - self.list.measure(gtk::Orientation::Vertical, width).1)
            .max(0) as f32;
        let unit_height = self
            .rows
            .borrow()
            .values()
            .filter(|row| row.id.get() != 0)
            .map(|row| row.root.measure(gtk::Orientation::Vertical, width).1)
            .max()
            .unwrap_or(layer_ui::TILE_SIZE as i32) as f32;
        let content_height =
            (fixed_height + unit_height * self.model.n_items() as f32).min(999_999.0);
        (
            content_height,
            layer_ui::PanelScrollMeasurement {
                fixed_height,
                unit_height,
            },
        )
    }

    pub fn bind(&self, w: &Rc<Workspace>) {
        let copy = self.copy.borrow();
        *self.owner.borrow_mut() = Rc::downgrade(w);
        w.watch_popover(self.context.upcast_ref());
        if self.root == w.layer_panel.root {
            let click = gtk::GestureClick::new();
            click.set_button(0);
            click.set_propagation_phase(gtk::PropagationPhase::Capture);
            for release in [false, true] {
                let close = glib::clone!(#[weak] w, move |g: &gtk::GestureClick, _: i32, x: f64, y: f64| {
                    let Some(widget) = g.widget() else { return; };
                    let extra: Vec<_> = w.drawers().iter().filter_map(|d| d.layers()).collect();
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
                                .collect::<Vec<_>>(),
                        );
                        glib::ControlFlow::Continue
                    }
                ),
            );
        }
        for (icon, label, a) in [
            (
                "layer-add-layer-symbolic",
                copy.layer.new_layer.as_ref(),
                A::New {
                    group: false,
                    clipped: false,
                },
            ),
            (
                "layer-folder-symbolic",
                copy.layer.new_group.as_ref(),
                A::New {
                    group: true,
                    clipped: false,
                },
            ),
        ] {
            let b = button(icon, label);
            b.set_widget_name(icon);
            w.bind_action_tooltip(&b, UiAction::Layer { action: a.clone() });
            b.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| action(&w, a.clone())
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
        self.footer.append(mask);
        let color_menu = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
        self.color_mode.set_popover(Some(&color_menu)); w.watch_popover(color_menu.upcast_ref());
        color_menu.connect_show(glib::clone!(#[weak] w, move |popover| {
            let menu = w.gpu.borrow().as_ref().and_then(|g| g.session.state().layer_tools.color_mode.as_ref().map(|control| control.menu.clone()));
            if let Some(menu) = menu { w.populate_workspace_menu(popover, menu); }
        }));
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
                if let Some(id) = active(&w) {
                    let mask = w.gpu.borrow().as_ref().is_some_and(|g| {
                        g.session
                            .state()
                            .layers
                            .iter()
                            .any(|l| l.id == id && l.mask_selected)
                    });
                    menu(
                        &w,
                        &context,
                        b.upcast_ref(),
                        id,
                        mask,
                        [0., b.height() as f64],
                    );
                }
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
            let delete = row.swipe.delete_button();
            let on_delete = picked.as_ref().is_some_and(|p| p == delete || p.is_ancestor(delete));
            if picked.is_none() || (release && !on_delete) { row.swipe.reveal(false); }
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
                "layer-folder-symbolic" => Some(&copy.layer.new_group),
                "import-image-layer" => Some(&copy.layer.import_image),
                "layer-actions" => Some(&copy.layer.actions),
                _ => None,
            };
            if let Some(label) = label { caption(&child, label); }
        }
        for i in 0..self.model.n_items() {
            if let Some(object) = self.model.item(i).and_downcast::<glib::BoxedAnyObject>() {
                let state = object.borrow::<LayerState>();
                for row in self.rows.borrow().values().filter(|row| row.id.get() == state.id) { row.refresh(&state); }
            }
        }
    }
    pub fn refresh(&self, state: &UiState) {
        // Update only changed rows; list virtualization bounds GTK widget count.
        let same = self.model.n_items() as usize == state.layers.len()
            && state.layers.iter().enumerate().all(|(i, l)| {
                self.model
                    .item(i as u32)
                    .and_downcast::<glib::BoxedAnyObject>()
                    .is_some_and(|o| o.borrow::<LayerState>().id == l.id)
            });
        if !same {
            let rows: Vec<_> = state
                .layers
                .iter()
                .cloned()
                .map(glib::BoxedAnyObject::new)
                .collect();
            self.model.splice(0, self.model.n_items(), &rows);
        } else {
            for (i, l) in state.layers.iter().enumerate() {
                let object = self
                    .model
                    .item(i as u32)
                    .and_downcast::<glib::BoxedAnyObject>()
                    .unwrap();
                if *object.borrow::<LayerState>() != *l {
                    *object.borrow_mut::<LayerState>() = l.clone();
                    for row in self
                        .rows
                        .borrow()
                        .values()
                        .filter(|row| row.id.get() == l.id)
                    {
                        row.refresh(l);
                    }
                }
            }
        }
        if let Some(l) = &state.layer_tools.editing_layer {
            self.opacity.set_value(l.opacity as f64);
            self.blend_label.set_text(&l.blend_label);
            self.alpha.set_active(l.alpha_locked);
            self.lock.set_active(l.locked);
        }
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
        self.color_mode.set_visible(state.layer_tools.color_mode.is_some());
        if let Some(control) = &state.layer_tools.color_mode {
            self.color_mode.set_label(&control.value); self.color_mode.set_sensitive(control.enabled); caption(&self.color_mode, &control.menu.title);
        }
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
    fn update_previews(&self, w: &Workspace, extra: &[Rc<Self>]) {
        let rows: Vec<_> = std::iter::once(self)
            .chain(extra.iter().map(|v| v.as_ref()))
            .filter(|v| v.root.is_mapped())
            .flat_map(|view| {
                view.rows
                    .borrow()
                    .values()
                    .filter(|r| {
                        r.root.compute_bounds(&view.list).is_some_and(|b| {
                                b.y() + b.height() > 0. && b.y() < view.list.height() as f32
                            })
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect();
        if rows.is_empty() {
            return;
        }
        let mut gpu = w.gpu.borrow_mut();
        let Some(g) = gpu.as_mut() else { return };
        if g.session.renderer_mut().ready().is_err() {
            return;
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
        for row in rows {
            let Some(state) = g
                .session
                .state()
                .layers
                .iter()
                .find(|l| l.id == row.id.get())
                .cloned()
            else {
                continue;
            };
            if state.group {
                continue;
            }
            for (mask, target, revision, picture) in [
                (
                    false,
                    state.has_thumbnail.then_some(state.id),
                    state.paint_revision,
                    &row.content_image,
                ),
                (true, state.mask_id, state.mask_revision, &row.mask_image),
            ] {
                let Some(target) = target else { continue };
                let key = (state.id, mask);
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
                        .insert(request, (state.id, mask, revision));
                }
            }
        }
    }
}
fn menu(
    w: &Rc<Workspace>,
    context: &gtk::PopoverMenu,
    anchor: &gtk::Widget,
    id: u64,
    mask: bool,
    [x, y]: [f64; 2],
) {
    action(w, A::Context { id, mask });
    let menu = w
        .gpu
        .borrow()
        .as_ref()
        .and_then(|g| g.session.layer_menu(id, mask).ok());
    let Some(menu) = menu else { return };
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
            self.content_image.set_paintable(None::<&gdk::Paintable>);
            self.mask_image.set_paintable(None::<&gdk::Paintable>);
        }
        self.root.set_widget_name(&format!("art-layer-{}", s.id));
        self.swipe.set_actions(s.can_delete, s.right_swipe.is_some());
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
        self.pass_through.set_visible(s.pass_through);
        self.load_selection.set_visible(s.selection_layer);
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
        self.content_frame
            .set_visible(s.content_selected);
        self.mask_frame.set_visible(s.mask_selected);
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
        self.link.set_visible(s.has_mask);
        self.mask.set_visible(s.has_mask);
        crate::icons::set_button(&self.link, if s.mask_linked { "layer-link-symbolic" } else { "layer-unlink-symbolic" });
        self.link.set_sensitive(!s.locked);
        caption(&self.link, if s.mask_linked { copy.layer.unlink_mask.as_ref() } else { copy.layer.link_mask_to_layer.as_ref() });
        self.mask_image
            .set_opacity(if s.mask_enabled { 1. } else { 0.4 });
        self.grip.set_visible(s.can_drop_below);
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
        self.meta.set_visible(!s.description.is_empty());
    }
}

#[cfg(test)]
mod copy_tests {
    use super::*;

    #[test]
    #[ignore = "private display native_layers_control_copy"]
    fn native_layers_control_copy() {
        unsafe { std::env::set_var("GTK_A11Y", "test"); }
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
