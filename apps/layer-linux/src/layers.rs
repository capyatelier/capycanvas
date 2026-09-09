//! Compact GTK layer list. Virtual rows render shared state and typed commands.
use crate::{number_control::NumberControl, workspace::Workspace};
use gtk::{gdk, gio, glib, prelude::*};
use layer_render::CanvasRenderer;
use layer_ui::{LayerAction as A, LayerCanvasTool, LayerState, NumericControl, UiAction, UiState};
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
    disclosure: gtk::Button,
    content: gtk::Button,
    mask: gtk::Button,
    link: gtk::Button,
    name: gtk::Label,
    meta: gtk::Label,
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
impl LayerPanel {
    pub fn new() -> Self {
        let owner: Rc<RefCell<Weak<Workspace>>> = Rc::default();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("layers-panel");
        let header = gtk::Box::new(gtk::Orientation::Vertical, 2);
        header.add_css_class("layer-header");
        root.append(&header);
        let options = gtk::Box::new(gtk::Orientation::Horizontal, 2);
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
        let alpha = gtk::ToggleButton::new();
        alpha.set_icon_name("changes-prevent-symbolic");
        alpha.add_css_class("layer-icon");
        alpha.set_tooltip_text(Some("Alpha lock"));
        options.append(&alpha);
        let clip = gtk::ToggleButton::new();
        clip.set_icon_name("layer-down-symbolic");
        clip.add_css_class("layer-icon");
        clip.set_tooltip_text(Some("Clip to layer below"));
        options.append(&clip);
        header.append(&options);
        let opacity = NumberControl::new(NumericControl::percent(), "Opacity", "");
        header.append(&opacity);
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
                let eye = button("view-reveal-symbolic", "Show layer");
                root.append(&eye);
                let disclosure = button("pan-down-symbolic", "Expand group");
                root.append(&disclosure);
                let content = button("layer-layers-symbolic", "Edit layer content");
                let content_image = gtk::Picture::new();
                content_image.set_can_shrink(true);
                content_image.set_content_fit(gtk::ContentFit::Contain);
                content_image.set_size_request(28, 28);
                content.set_child(Some(&content_image));
                content.add_css_class("layer-thumbnail");
                root.append(&content);
                let link = button("insert-link-symbolic", "Link mask to layer");
                link.add_css_class("layer-link");
                root.append(&link);
                let mask = button("image-x-generic-symbolic", "Edit layer mask");
                let mask_image = gtk::Picture::new();
                mask_image.set_can_shrink(true);
                mask_image.set_content_fit(gtk::ContentFit::Contain);
                mask_image.set_size_request(28, 28);
                mask.set_child(Some(&mask_image));
                mask.add_css_class("layer-thumbnail");
                mask.add_css_class("mask-thumbnail");
                root.append(&mask);
                let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
                text.set_hexpand(true);
                text.set_valign(gtk::Align::Center);
                let name = gtk::Label::new(None);
                name.set_xalign(0.);
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                name.set_width_chars(1);
                name.set_hexpand(true);
                text.append(&name);
                let meta = gtk::Label::new(None);
                meta.add_css_class("dim-label");
                meta.set_xalign(0.);
                meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
                meta.set_width_chars(1);
                text.append(&meta);
                root.append(&text);
                let grip = gtk::Image::from_icon_name("layer-grip-symbolic");
                grip.set_pixel_size(12);
                grip.add_css_class("dim-label");
                root.append(&grip);
                for (b, kind) in [
                    (&eye, 0),
                    (&disclosure, 1),
                    (&content, 2),
                    (&link, 3),
                    (&mask, 4),
                ] {
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
                            if kind == 0 {
                                w.dispatch(UiAction::SetLayerVisibility {
                                    id: row.id,
                                    visible: !row.visible,
                                });
                                return;
                            }
                            let a = match kind {
                                1 => A::Collapse { id: row.id },
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
                            };
                            action(&w, a);
                        }
                    ));
                }
                let click = gtk::GestureClick::new();
                click.set_button(1);
                click.connect_released(glib::clone!(
                    #[weak]
                    item,
                    #[strong]
                    owner,
                    move |_, n, _, _| {
                        let Some(row) = row_state(&item) else { return };
                        let Some(w) = owner.borrow().upgrade() else {
                            return;
                        };
                        if n == 2 {
                            w.layer_panel.rename(&w, &row);
                        } else if !row.selected {
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
                text.add_controller(click);
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
                let drag = gtk::DragSource::new();
                drag.set_actions(gdk::DragAction::MOVE);
                drag.connect_prepare(glib::clone!(
                    #[weak]
                    item,
                    #[upgrade_or]
                    None,
                    move |_, _, _| Some(gdk::ContentProvider::for_value(
                        &format!("capy-layer:{}", row_state(&item)?.id).to_value()
                    ))
                ));
                grip.add_controller(drag);
                let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
                drop.connect_enter(glib::clone!(
                    #[weak]
                    root,
                    #[upgrade_or]
                    gdk::DragAction::empty(),
                    move |_, _, y| {
                        root.add_css_class(if y < root.height() as f64 / 2. {
                            "layer-drop-before"
                        } else {
                            "layer-drop-after"
                        });
                        gdk::DragAction::MOVE
                    }
                ));
                drop.connect_leave(glib::clone!(
                    #[weak]
                    root,
                    move |_| {
                        root.remove_css_class("layer-drop-before");
                        root.remove_css_class("layer-drop-after");
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
                        disclosure,
                        content,
                        mask,
                        link,
                        name,
                        meta,
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
                "list-add-symbolic",
                "New layer",
                A::New {
                    group: false,
                    clipped: false,
                },
            ),
            (
                "folder-new-symbolic",
                "New group",
                A::New {
                    group: true,
                    clipped: false,
                },
            ),
            (
                "edit-select-all-symbolic",
                "Lasso selection",
                A::Tool {
                    tool: LayerCanvasTool::Select,
                },
            ),
            (
                "layer-move-symbolic",
                "Move layer or mask",
                A::Tool {
                    tool: LayerCanvasTool::Move,
                },
            ),
        ] {
            let b = button(icon, label);
            b.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| action(&w, a.clone())
            ));
            self.footer.append(&b);
        }
        let mask = button(
            "image-x-generic-symbolic",
            "Add mask from selection, or reveal all",
        );
        mask.connect_clicked(glib::clone!(
            #[weak]
            w,
            move |_| {
                if let Some(id) = active(&w) {
                    action(&w, A::AddMask { id, replace: false });
                }
            }
        ));
        self.footer.append(&mask);
        let import = button("document-open-symbolic", "Import image as layer");
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
        let more = button("view-more-symbolic", "Layer actions");
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
                    self.model
                        .splice(i as u32, 1, &[glib::BoxedAnyObject::new(l.clone())]);
                }
            }
        }
        if let Some(l) = state.layers.iter().find(|l| l.selected) {
            self.opacity.set_value(l.opacity as f64);
            self.blend.set_selected(l.blend);
            self.alpha.set_active(l.alpha_locked);
            self.alpha.set_sensitive(l.editable);
            self.clip.set_active(l.clipped);
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
        action(w, A::Select { id, mask });
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
    fn rename(&self, w: &Rc<Workspace>, row: &LayerState) {
        let popup = gtk::Popover::new();
        popup.set_parent(&self.root);
        let entry = gtk::Entry::builder()
            .text(&row.label)
            .max_length(128)
            .build();
        popup.set_child(Some(&entry));
        let id = row.id;
        entry.connect_activate(glib::clone!(
            #[weak]
            w,
            #[weak]
            popup,
            move |e| {
                action(
                    &w,
                    A::Rename {
                        id,
                        name: e.text().into(),
                    },
                );
                popup.popdown();
            }
        ));
        popup.connect_closed(|p| p.unparent());
        popup.popup();
        entry.grab_focus();
        entry.select_region(0, -1);
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
        .layers
        .iter()
        .find(|l| l.selected)
        .map(|l| l.id)
}
impl Row {
    fn refresh(&self, s: &LayerState) {
        if self.id.replace(s.id) != s.id {
            self.content_image.set_paintable(None::<&gdk::Paintable>);
            self.mask_image.set_paintable(None::<&gdk::Paintable>);
        }
        self.root.set_widget_name(&format!("art-layer-{}", s.id));
        self.name.set_text(&s.label);
        self.name.set_tooltip_text(Some(&s.label));
        self.root.set_margin_start((s.depth * 8).min(32) as i32);
        for (w, selected) in [
            (self.root.clone().upcast::<gtk::Widget>(), s.selected),
            (
                self.content.clone().upcast(),
                s.selected && !s.mask_selected,
            ),
            (self.mask.clone().upcast(), s.mask_selected),
        ] {
            if selected {
                w.add_css_class("selected")
            } else {
                w.remove_css_class("selected")
            }
        }
        self.disclosure.set_visible(s.group);
        self.disclosure.set_icon_name(if s.collapsed {
            "pan-end-symbolic"
        } else {
            "pan-down-symbolic"
        });
        self.eye.set_icon_name(if s.visible {
            "view-reveal-symbolic"
        } else {
            "view-conceal-symbolic"
        });
        self.link.set_visible(s.has_mask);
        self.mask.set_visible(s.has_mask);
        self.link.set_icon_name("insert-link-symbolic");
        self.link.set_opacity(if s.mask_linked { 1. } else { 0.35 });
        self.link.set_tooltip_text(Some(if s.mask_linked {
            "Unlink mask from layer"
        } else {
            "Link mask to layer"
        }));
        self.mask.set_opacity(if s.mask_enabled { 1. } else { 0.4 });
        self.grip.set_visible(s.editable || s.group);
        if s.group {
            self.content.set_icon_name("folder-symbolic");
        } else {
            self.content.set_child(Some(&self.content_image));
        }
        let mut parts = Vec::new();
        if s.reference {
            parts.push("Reference".into());
        }
        if s.locked {
            parts.push("Locked".into());
        }
        if s.alpha_locked {
            parts.push("α".into());
        }
        if s.clipped {
            parts.push("Clipped".into());
        }
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
