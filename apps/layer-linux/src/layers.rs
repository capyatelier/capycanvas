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

pub struct LayerPanel {
    pub root: gtk::Box,
    pub header: gtk::Box,
    pub footer: gtk::Box,
    pub list: gtk::ScrolledWindow,
    pub opacity: NumberControl,
    model: gio::ListStore,
    blend: gtk::DropDown,
    alpha: gtk::ToggleButton,
    lock: gtk::ToggleButton,
    mask_action: gtk::Button,
    reference: gtk::ToggleButton,
    clip: gtk::ToggleButton,
    context: gtk::PopoverMenu,
    owner: Rc<RefCell<Weak<Workspace>>>,
    rows: Rc<RefCell<HashMap<usize, Row>>>,
    previews: RefCell<HashMap<(u64, bool), (u64, gdk::MemoryTexture)>>,
    requested: RefCell<HashMap<(u64, bool), u64>>,
    pending: RefCell<HashMap<u64, (u64, bool, u64)>>,
    next_preview: Cell<u64>,
    updating: Cell<bool>,
}
#[derive(Clone)]
struct Row {
    id: Cell<u64>,
    content_image: gtk::Picture,
    mask_image: gtk::Picture,
    root: gtk::Box,
    eye: gtk::Button,
    selection: gtk::Button,
    thumbnails: gtk::Box,
    clipping: gtk::Box,
    content: gtk::Button,
    content_preview: gtk::Overlay,
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
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("flat");
    b.add_css_class("layer-icon");
    b.set_tooltip_text(Some(tooltip));
    b
}
fn action(w: &Workspace, action: A) {
    if let Some(w) = w.layer_panel.owner.borrow().upgrade() {
        w.dispatch(UiAction::Layer { action });
    }
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
        for (x, y, dx, dy) in [
            (1.5, 1.5, 1., 1.),
            (w - 1.5, 1.5, -1., 1.),
            (1.5, h - 1.5, 1., -1.),
            (w - 1.5, h - 1.5, -1., -1.),
        ] {
            cr.move_to(x, y + dy * 6.);
            cr.line_to(x, y);
            cr.line_to(x + dx * 6., y);
        }
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
    button.set_icon_name(icon);
    button.add_css_class("flat");
    button.add_css_class("layer-icon");
    button.set_tooltip_text(Some(tooltip));
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
    touch: bool,
    owner: &Rc<RefCell<Weak<Workspace>>>,
) -> gtk::DragSource {
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    source.set_propagation_phase(gtk::PropagationPhase::Capture);
    source.connect_prepare(glib::clone!(
        #[weak]
        item,
        #[weak]
        root,
        #[weak]
        name,
        #[strong]
        owner,
        #[upgrade_or]
        None,
        move |source, x, y| {
            if name.visible_child_name().as_deref() == Some("edit")
                || (!touch
                    && source
                        .current_event_device()
                        .is_some_and(|d| d.source() == gdk::InputSource::Touchscreen))
            {
                return None;
            }
            let row = row_state(&item)?;
            if !row.editable && !row.group {
                return None;
            }
            let w = owner.borrow().upgrade()?;
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
    snapshot.to_paintable(Some(&gtk::graphene::Size::new(
        row.width() as f32,
        row.height() as f32,
    )))
}
fn drop_hint(root: &gtk::Box, state: Option<LayerState>, y: f64) {
    for class in ["layer-drop-before", "layer-drop-after", "layer-drop-into"] {
        root.remove_css_class(class);
    }
    let group = state.as_ref().is_some_and(|s| s.group);
    let fraction = if state.as_ref().is_some_and(|s| s.can_drop_below) {
        y / root.height().max(1) as f64
    } else {
        0.
    };
    root.add_css_class(if group && (0.25..0.75).contains(&fraction) {
        "layer-drop-into"
    } else if fraction < 0.5 {
        "layer-drop-before"
    } else {
        "layer-drop-after"
    });
}
fn row_button_action(row: &LayerState, kind: u8) -> UiAction {
    if kind == 0 {
        return UiAction::SetLayerVisibility {
            id: row.id,
            visible: !row.visible,
        };
    }
    UiAction::Layer {
        action: match kind {
            1 => A::ToggleSelection { id: row.id },
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
    pub fn new() -> Self {
        let owner: Rc<RefCell<Weak<Workspace>>> = Rc::default();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("layers-panel");
        let header = gtk::Box::new(gtk::Orientation::Vertical, 2);
        header.add_css_class("layer-header");
        root.append(&header);
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        options.set_homogeneous(true);
        let labels: Vec<_> = layer_core::LayerBlend::ALL
            .iter()
            .map(|b| b.label())
            .collect();
        let blend = gtk::DropDown::from_strings(&labels);
        let compact = gtk::SignalListItemFactory::new();
        compact.connect_setup(|_, item| {
            let label = gtk::Label::new(None);
            label.set_xalign(0.);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_max_width_chars(10);
            label.set_width_chars(1);
            item.downcast_ref::<gtk::ListItem>()
                .unwrap()
                .set_child(Some(&label));
        });
        compact.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            if let (Some(label), Some(value)) = (
                item.child().and_downcast::<gtk::Label>(),
                item.item().and_downcast::<gtk::StringObject>(),
            ) {
                label.set_text(&value.string());
            }
        });
        blend.set_factory(Some(&compact));
        blend.set_hexpand(true);
        blend.set_tooltip_text(Some("Layer blend mode"));
        options.append(&blend);
        let opacity = NumberControl::inline(NumericControl::layer_opacity(), "Layer opacity");
        options.append(&opacity);
        header.append(&options);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let alpha = toggle("layer-alpha-lock-symbolic", "Alpha lock");
        let lock = toggle("layer-lock-symbolic", "Lock editing");
        let clip = toggle("layer-clip-symbolic", "Clip to layer below");
        let reference = toggle(
            "layer-reference-symbolic",
            "Use selected layers as references",
        );
        reference.add_css_class("layer-reference");
        for b in [&alpha, &lock, &clip, &reference] {
            actions.append(b);
        }
        header.append(&actions);
        let model = gio::ListStore::new::<glib::BoxedAnyObject>();
        let factory = gtk::SignalListItemFactory::new();
        let rows: Rc<RefCell<HashMap<usize, Row>>> = Rc::default();
        factory.connect_setup(glib::clone!(
            #[strong]
            rows,
            #[strong]
            owner,
            move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let root = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                root.add_css_class("layer-row");
                root.add_css_class("customizable-target");
                let eye = button("layer-eye-symbolic", "Show layer");
                eye.add_css_class("layer-column");
                root.append(&eye);
                let selection = button(
                    "layer-selection-empty-symbolic",
                    "Select layer without changing drawing target",
                );
                selection.add_css_class("layer-column");
                root.append(&selection);
                let thumbnails = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                let clipping = gtk::Box::new(gtk::Orientation::Vertical, 0);
                clipping.add_css_class("layer-clipping");
                thumbnails.append(&clipping);
                let (content, content_image, content_preview, content_frame) =
                    thumbnail("Edit layer content");
                thumbnails.append(&content);
                let link = button("layer-link-symbolic", "Link mask to layer");
                link.add_css_class("layer-link");
                thumbnails.append(&link);
                let (mask, mask_image, _, mask_frame) = thumbnail("Edit layer mask");
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
                name_entry.add_css_class("layer-name-entry");
                let name_stack = gtk::Stack::new();
                name_stack.set_hhomogeneous(false);
                name_stack.set_vhomogeneous(false);
                name_stack.add_named(&name, Some("name"));
                name_stack.add_named(&name_entry, Some("edit"));
                text.append(&name_stack);
                let meta = gtk::Label::new(None);
                meta.add_css_class("dim-label");
                meta.set_xalign(0.);
                meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
                meta.set_width_chars(1);
                text.append(&meta);
                root.append(&text);
                let lock = gtk::Image::new();
                lock.set_pixel_size(12);
                lock.set_size_request(12, -1);
                root.append(&lock);
                let grip = gtk::Image::from_icon_name("layer-grip-symbolic");
                grip.set_pixel_size(12);
                grip.add_css_class("dim-label");
                root.append(&grip);
                for (b, kind) in [
                    (&eye, 0),
                    (&selection, 1),
                    (&content, 2),
                    (&link, 3),
                    (&mask, 4),
                ] {
                    b.connect_query_tooltip(glib::clone!(
                        #[weak]
                        item,
                        #[strong]
                        owner,
                        #[upgrade_or]
                        false,
                        move |button, _, _, _, tooltip| {
                            let Some(row) = row_state(&item) else {
                                return false;
                            };
                            let Some(w) = owner.borrow().upgrade() else {
                                return false;
                            };
                            let gpu = w.gpu.borrow();
                            let Some(g) = gpu.as_ref() else {
                                return false;
                            };
                            let state = g.session.state();
                            tooltip.set_text(Some(&state.settings.action_tooltip(
                                &button.tooltip_text().unwrap_or_default(),
                                &row_button_action(&row, kind),
                                state.platform,
                            )));
                            true
                        }
                    ));
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
                        }
                    ));
                }
                let click = gtk::GestureClick::new();
                click.set_button(1);
                click.connect_released(glib::clone!(
                    #[weak]
                    item,
                    #[weak]
                    root,
                    #[weak]
                    name,
                    #[strong]
                    owner,
                    move |_, n, x, y| {
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
                        if n == 2 && picked.as_ref() == Some(name.upcast_ref()) {
                            action(&w, A::BeginRename { id: row.id });
                        } else if !row.editing || !row.selected {
                            action(
                                &w,
                                A::Select {
                                    id: row.id,
                                    mask: false,
                                },
                            );
                        }
                    }
                ));
                root.add_controller(click);
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
                            finish_name(&owner, &item, &name_stack, &name_entry, true);
                            glib::Propagation::Stop
                        } else {
                            glib::Propagation::Proceed
                        }
                    }
                ));
                name_entry.add_controller(keys);
                for (widget, is_mask) in [
                    (root.clone().upcast::<gtk::Widget>(), false),
                    (mask.clone().upcast(), true),
                ] {
                    let click = gtk::GestureClick::new();
                    click.set_button(3);
                    click.connect_pressed(glib::clone!(
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
                            w.layer_panel
                                .menu(&w, &g.widget().unwrap(), row.id, is_mask, x, y);
                        }
                    ));
                    widget.add_controller(click);
                    let hold = gtk::GestureLongPress::new();
                    hold.set_touch_only(true);
                    hold.connect_pressed(glib::clone!(
                        #[weak]
                        item,
                        #[strong]
                        owner,
                        move |g, x, y| {
                            let Some(row) = row_state(&item) else { return };
                            let Some(w) = owner.borrow().upgrade() else {
                                return;
                            };
                            g.set_state(gtk::EventSequenceState::Claimed);
                            w.layer_panel
                                .menu(&w, &g.widget().unwrap(), row.id, is_mask, x, y);
                        }
                    ));
                    widget.add_controller(hold);
                }
                for (widget, touch) in [
                    (root.clone().upcast::<gtk::Widget>(), false),
                    (grip.clone().upcast(), true),
                ] {
                    widget.add_controller(row_drag(item, &root, &name_stack, touch, &owner));
                }
                let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
                drop.connect_enter(glib::clone!(
                    #[weak]
                    item,
                    #[weak]
                    root,
                    #[upgrade_or]
                    gdk::DragAction::empty(),
                    move |_, _, y| {
                        drop_hint(&root, row_state(&item), y);
                        gdk::DragAction::MOVE
                    }
                ));
                drop.connect_motion(glib::clone!(
                    #[weak]
                    root,
                    #[weak]
                    item,
                    #[upgrade_or]
                    gdk::DragAction::empty(),
                    move |_, _, y| {
                        drop_hint(&root, row_state(&item), y);
                        gdk::DragAction::MOVE
                    }
                ));
                drop.connect_leave(glib::clone!(
                    #[weak]
                    root,
                    move |_| {
                        root.remove_css_class("layer-drop-before");
                        root.remove_css_class("layer-drop-after");
                        root.remove_css_class("layer-drop-into");
                    }
                ));
                drop.connect_drop(glib::clone!(
                    #[weak]
                    item,
                    #[weak]
                    root,
                    #[strong]
                    owner,
                    #[upgrade_or]
                    false,
                    move |_, value, _, y| {
                        root.remove_css_class("layer-drop-before");
                        root.remove_css_class("layer-drop-after");
                        root.remove_css_class("layer-drop-into");
                        let Some(id) = value.get::<String>().ok().and_then(|s| {
                            s.strip_prefix("capy-layer:").and_then(|s| s.parse().ok())
                        }) else {
                            return false;
                        };
                        let Some(row) = row_state(&item) else {
                            return false;
                        };
                        let Some(w) = owner.borrow().upgrade() else {
                            return false;
                        };
                        action(
                            &w,
                            A::Drop {
                                id,
                                target: row.id,
                                fraction: (y / root.height().max(1) as f64) as f32,
                            },
                        );
                        true
                    }
                ));
                root.add_controller(drop);
                item.set_child(Some(&root));
                rows.borrow_mut().insert(
                    item.as_ptr() as usize,
                    Row {
                        id: Cell::new(0),
                        content_image,
                        mask_image,
                        root,
                        eye,
                        selection,
                        thumbnails,
                        clipping,
                        content,
                        content_preview,
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
            #[strong]
            rows,
            move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().unwrap();
                let Some(state) = row_state(item) else { return };
                let rows = rows.borrow();
                let row = &rows[&(item.as_ptr() as usize)];
                row.refresh(&state);
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
                    row.id.set(0);
                }
            }
        ));
        let selection = gtk::NoSelection::new(Some(model.clone()));
        let view = gtk::ListView::new(Some(selection), Some(factory));
        view.set_single_click_activate(false);
        view.add_css_class("layer-list");
        let list = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .min_content_height(72)
            .child(&view)
            .build();
        root.append(&list);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        footer.add_css_class("layer-footer");
        root.append(&footer);
        let context = gtk::PopoverMenu::from_model(None::<&gio::Menu>);
        context.set_parent(&root);
        context.set_has_arrow(false);
        Self {
            root,
            header,
            footer,
            list,
            opacity,
            model,
            blend,
            alpha,
            lock,
            mask_action: button(
                "layer-mask-symbolic",
                "Add mask from selection, or reveal all",
            ),
            reference,
            clip,
            context,
            owner,
            rows,
            previews: Default::default(),
            requested: Default::default(),
            pending: Default::default(),
            next_preview: Cell::new(1),
            updating: Cell::new(false),
        }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        *self.owner.borrow_mut() = Rc::downgrade(w);
        glib::timeout_add_local(
            std::time::Duration::from_millis(120),
            glib::clone!(
                #[weak]
                w,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    w.layer_panel.update_previews(&w);
                    glib::ControlFlow::Continue
                }
            ),
        );
        for (icon, label, a) in [
            (
                "layer-plus-symbolic",
                "New layer",
                A::New {
                    group: false,
                    clipped: false,
                },
            ),
            (
                "layer-folder-symbolic",
                "New group",
                A::New {
                    group: true,
                    clipped: false,
                },
            ),
        ] {
            let b = button(icon, label);
            w.bind_action_tooltip(&b, UiAction::Layer { action: a.clone() });
            b.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| action(&w, a.clone())
            ));
            self.footer.append(&b);
        }
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
        for (button, kind) in [(&self.alpha, 0), (&self.lock, 1), (&self.clip, 2)] {
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
                        _ => A::Clip {
                            id,
                            value: !row.clipped,
                        },
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
        let import = button("layer-image-symbolic", "Import image as layer");
        import.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                let dialog = gtk::FileDialog::builder()
                    .title("Import image as layer")
                    .build();
                dialog.open(
                    Some(&w.window),
                    None::<&gio::Cancellable>,
                    glib::clone!(
                        #[weak]
                        w,
                        move |result| {
                            let Ok(file) = result else {
                                return;
                            };
                            let result = (|| -> Result<(), String> {
                                let texture =
                                    gdk::Texture::from_file(&file).map_err(|e| e.to_string())?;
                                let mut download = gdk::TextureDownloader::new(&texture);
                                download.set_format(gdk::MemoryFormat::R8g8b8a8);
                                let (bytes, stride) = download.download_bytes();
                                let name = file
                                    .basename()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "Image".into());
                                w.gpu
                                    .borrow_mut()
                                    .as_mut()
                                    .ok_or("Canvas unavailable")?
                                    .session
                                    .import_layer_image(
                                        &name,
                                        layer_render::HostImage {
                                            width: texture.width() as u32,
                                            height: texture.height() as u32,
                                            stride: stride as u32,
                                            format: layer_render::PixelFormat::Rgba8Srgb,
                                            bytes: &bytes,
                                        },
                                    )
                            })();
                            w.changed(result.map(|_| layer_ui::UiChange {
                                revision: w.gpu.borrow().as_ref().unwrap().session.state().revision,
                                regions: layer_ui::regions::DOCUMENT,
                                canvas_wake: true,
                            }));
                        }
                    ),
                );
            }
        ));
        self.footer.append(&import);
        let more = button("layer-more-symbolic", "Layer actions");
        more.set_hexpand(true);
        more.set_halign(gtk::Align::End);
        more.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |b| {
                if w.layer_panel.updating.get() {
                    return;
                }
                if let Some(id) = active(&w) {
                    let mask = w.gpu.borrow().as_ref().is_some_and(|g| {
                        g.session
                            .state()
                            .layers
                            .iter()
                            .any(|l| l.id == id && l.mask_selected)
                    });
                    w.layer_panel
                        .menu(&w, b.upcast_ref(), id, mask, 0., b.height() as f64);
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
        self.blend.connect_selected_notify(glib::clone!(
            #[weak]
            w,
            move |b| {
                if let Some(id) = active(&w) {
                    action(
                        &w,
                        A::Blend {
                            id,
                            value: b.selected(),
                        },
                    );
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
        self.clip.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |b| {
                if let Some(id) = active(&w) {
                    action(
                        &w,
                        A::Clip {
                            id,
                            value: b.is_active(),
                        },
                    );
                }
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
    pub fn refresh(&self, state: &UiState) {
        self.updating.set(true);
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
            self.blend.set_selected(l.blend);
            self.alpha.set_active(l.alpha_locked);
            self.lock.set_active(l.locked);
            self.clip.set_active(l.clipped);
        }
        let controls = state.layer_tools.controls;
        self.opacity.set_sensitive(controls.opacity);
        self.blend.set_sensitive(controls.blend);
        self.alpha.set_sensitive(controls.alpha_lock);
        self.lock.set_sensitive(controls.edit_lock);
        self.clip.set_sensitive(controls.clip);
        self.mask_action.set_sensitive(controls.mask);
        self.reference
            .set_active(state.layer_tools.references_selected);
        self.reference
            .set_sensitive(state.layer_tools.can_reference);
        self.reference
            .set_tooltip_text(Some(state.layer_tools.reference_action_label));
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
        self.updating.set(false);
    }
    fn update_previews(&self, w: &Workspace) {
        if !self.root.is_mapped() {
            return;
        }
        let rows: Vec<_> = self
            .rows
            .borrow()
            .values()
            .filter(|r| {
                r.id.get() != 0
                    && r.root.compute_bounds(&self.list).is_some_and(|b| {
                        b.y() + b.height() > 0. && b.y() < self.list.height() as f32
                    })
            })
            .cloned()
            .collect();
        let mut gpu = w.gpu.borrow_mut();
        let Some(g) = gpu.as_mut() else { return };
        if g.session.renderer_mut().ready().is_err() {
            return;
        }
        while let Some(result) = g.session.renderer_mut().take_thumbnail() {
            if let Ok(image) = result
                && let Some((id, mask, revision)) =
                    self.pending.borrow_mut().remove(&image.request_id)
                && self.requested.borrow().get(&(id, mask)) == Some(&revision)
            {
                let texture = gdk::MemoryTexture::new(
                    image.width as i32,
                    image.height as i32,
                    gdk::MemoryFormat::R8g8b8a8,
                    &glib::Bytes::from_owned(image.bytes),
                    image.stride as usize,
                );
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
                    Some(state.id),
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
                    .request_thumbnail(request, layer_core::LayerId(target))
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
    fn menu(&self, w: &Rc<Workspace>, anchor: &gtk::Widget, id: u64, mask: bool, x: f64, y: f64) {
        action(w, A::Context { id, mask });
        let menu = w
            .gpu
            .borrow()
            .as_ref()
            .and_then(|g| g.session.layer_menu(id, mask).ok());
        let Some(menu) = menu else { return };
        w.populate_workspace_menu(&self.context, menu);
        if let Some(p) =
            anchor.compute_point(&self.root, &gtk::graphene::Point::new(x as f32, y as f32))
        {
            self.context.set_pointing_to(Some(&gdk::Rectangle::new(
                p.x() as i32,
                p.y() as i32,
                1,
                1,
            )));
        }
        self.context.popup();
    }
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
        if self.id.replace(s.id) != s.id {
            self.name_stack.set_visible_child_name("name");
            self.name_entry.set_text("");
            self.content_image.set_paintable(None::<&gdk::Paintable>);
            self.mask_image.set_paintable(None::<&gdk::Paintable>);
        }
        self.root.set_widget_name(&format!("art-layer-{}", s.id));
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
            .set_visible(s.editing && !s.mask_selected);
        self.mask_frame.set_visible(s.mask_selected);
        let drawing_target = s.editing && (s.editable || s.mask_selected);
        self.selection.set_icon_name(s.selection_icon);
        self.selection
            .set_tooltip_text(Some(if drawing_target && s.reference {
                "Drawing target · Reference layer · Click to select"
            } else if drawing_target {
                "Drawing target · Click to select"
            } else if s.reference {
                "Reference layer · Click to select"
            } else {
                "Select layer without changing drawing target"
            }));
        self.clipping.set_opacity(if s.clipped { 1. } else { 0. });
        self.eye.set_icon_name(if s.visible {
            "layer-eye-symbolic"
        } else {
            "layer-eye-hidden-symbolic"
        });
        self.eye.set_tooltip_text(Some(if s.visible {
            "Hide layer"
        } else {
            "Show layer"
        }));
        self.link.set_visible(s.has_mask);
        self.mask.set_visible(s.has_mask);
        self.link.set_icon_name("layer-link-symbolic");
        self.link.set_opacity(if s.mask_linked { 1. } else { 0.35 });
        self.link.set_tooltip_text(Some(if s.mask_linked {
            "Unlink mask from layer"
        } else {
            "Link mask to layer"
        }));
        self.mask_image
            .set_opacity(if s.mask_enabled { 1. } else { 0.4 });
        self.grip.set_visible(s.editable || s.group);
        if s.group {
            self.content.add_css_class("layer-folder");
            self.content.set_icon_name(if s.collapsed {
                "layer-folder-symbolic"
            } else {
                "layer-folder-open-symbolic"
            });
            self.content.set_tooltip_text(Some(if s.collapsed {
                "Expand group"
            } else {
                "Collapse group"
            }));
        } else {
            self.content.remove_css_class("layer-folder");
            self.content.set_child(Some(&self.content_preview));
            self.content.set_tooltip_text(Some(if s.editable {
                "Edit layer content"
            } else {
                "Select paper"
            }));
        }
        self.lock.set_icon_name(Some(if s.locked {
            "layer-lock-symbolic"
        } else {
            "layer-alpha-lock-symbolic"
        }));
        self.lock
            .set_opacity(if s.locked || s.alpha_locked { 1. } else { 0. });
        self.lock.set_tooltip_text(Some(if s.locked {
            "Editing locked"
        } else {
            "Alpha locked"
        }));
        let mut parts = Vec::new();
        if s.blend != 0 {
            parts.push(s.blend_label.clone());
        }
        if s.opacity < 1. {
            parts.push(format!("{}%", (s.opacity * 100.).round() as u32));
        }
        self.meta.set_text(&parts.join(" · "));
        self.meta.set_visible(!parts.is_empty());
    }
}
