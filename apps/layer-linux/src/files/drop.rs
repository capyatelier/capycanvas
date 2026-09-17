//! Native file-list receipt. Capture document identity, destination and the
//! camera-derived point before file preparation; shared Rust owns placement.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use layer_ui::{CommandId, ImageLayerDestination, LayerDropPosition, UiAction};
use std::rc::Rc;

pub(crate) struct Incoming {
    pub files: Vec<gio::File>,
    pub center: Option<layer_core::Point>,
    pub destination: Option<ImageLayerDestination>,
    pub epoch: u64,
    pub revision: u64,
    pub target: layer_core::LayerId,
}
fn project(file: &gio::File) -> bool {
    file.path()
        .and_then(|p| p.extension().map(|s| s.eq_ignore_ascii_case("capy")))
        .unwrap_or(false)
}
fn kind(files: &[gio::File]) -> Result<bool, String> {
    if files.is_empty() {
        return Err("No files to import".into());
    }
    let projects = files.iter().filter(|f| project(f)).count();
    if projects > 0 && files.len() != 1 {
        return Err(
            "Open drawings one at a time; drop a batch containing only images to add layers".into(),
        );
    }
    Ok(projects == 1)
}
fn available(w: &Workspace, open: bool) -> bool {
    w.image_drop.borrow().is_none()
        && w.gpu.borrow().as_ref().is_some_and(|g| {
            g.session
                .command(if open {
                    CommandId::OpenDocument
                } else {
                    CommandId::ImportImage
                })
                .enabled
        })
}
fn files(target: &gtk::DropTarget) -> Option<Vec<gio::File>> {
    Some(target.value()?.get::<gdk::FileList>().ok()?.files())
}
fn feedback_kind(target: &gtk::DropTarget) -> Option<bool> {
    files(target).map_or(Some(false), |files| kind(&files).ok())
}
pub(crate) fn receive(
    w: &Rc<Workspace>,
    files: Vec<gio::File>,
    center: Option<layer_core::Point>,
    destination: Option<ImageLayerDestination>,
) -> bool {
    let result = (|| {
        let open = kind(&files)?;
        if open && destination.is_some() {
            return Err("Drop drawing files on the canvas to open them".into());
        }
        if !available(w, open) {
            return Err("Finish the current operation before dropping files".into());
        }
        let mut gpu = w.gpu.borrow_mut();
        let g = gpu.as_mut().ok_or("Canvas unavailable")?;
        let doc = g.session.engine().document();
        *w.image_drop.borrow_mut() = Some(Incoming {
            files,
            center,
            destination,
            epoch: g.session.state().document_file.epoch,
            revision: doc.revision,
            target: doc.active_target(),
        });
        g.session.dispatch(UiAction::Invoke {
            command: if open {
                CommandId::OpenDocument
            } else {
                CommandId::ImportImage
            },
        })
    })();
    let accepted = result.is_ok();
    if !accepted {
        w.image_drop.borrow_mut().take();
    }
    w.changed(result);
    accepted
}
pub(crate) fn install(w: &Rc<Workspace>) {
    let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    target.set_preload(true);
    let feedback = glib::clone!(
        #[weak]
        w,
        #[upgrade_or]
        gdk::DragAction::empty(),
        move |target: &gtk::DropTarget, _: f64, _: f64| {
            let kind = files(target).map_or(Ok(false), |files| kind(&files));
            let valid = kind.as_ref().is_ok_and(|open| available(&w, *open));
            w.image_drop_label.set_label(match &kind {
                Ok(true) => "Open drawing",
                Ok(false) => "Add images as layers",
                Err(reason) => reason,
            });
            // A forbidden cursor alone cannot explain an unsupported batch.
            // Keep its reason visible while the pointer is over the canvas.
            w.image_drop_label.set_visible(valid || kind.is_err());
            if valid {
                gdk::DragAction::COPY
            } else {
                gdk::DragAction::empty()
            }
        }
    );
    target.connect_enter(feedback.clone());
    target.connect_motion(feedback.clone());
    target.connect_value_notify(glib::clone!(
        #[weak]
        w,
        move |target| {
            if let Some(drop) = target.current_drop() {
                let action = feedback(target, 0., 0.);
                drop.status(action, action);
            } else {
                w.image_drop_label.set_visible(false);
            }
        }
    ));
    target.connect_leave(glib::clone!(
        #[weak]
        w,
        move |_| w.image_drop_label.set_visible(false)
    ));
    target.connect_drop(glib::clone!(
        #[weak]
        w,
        #[upgrade_or]
        false,
        move |_, value, x, y| {
            w.image_drop_label.set_visible(false);
            let Ok(files) = value.get::<gdk::FileList>() else {
                return false;
            };
            let center = {
                let gpu = w.gpu.borrow();
                let Some(g) = gpu.as_ref() else {
                    return false;
                };
                let scale = w.area.scale_factor() as f32;
                g.session
                    .state()
                    .camera
                    .input_transform()
                    .map(layer_core::Point {
                        x: x as f32 * scale,
                        y: y as f32 * scale,
                    })
            };
            receive(&w, files.files(), Some(center), None)
        }
    ));
    w.area.add_controller(target);
}

pub(crate) fn clear_row(root: &gtk::Box) {
    for class in ["layer-drop-before", "layer-drop-after", "layer-drop-into"] {
        root.remove_css_class(class);
    }
}
/// The resolver reads the currently bound virtual row each time. Retained rows
/// and drawer copies therefore cannot keep the identity of an earlier item.
pub(crate) fn install_row(
    root: &gtk::Box,
    resolve: impl Fn() -> Option<(Rc<Workspace>, u64)> + 'static,
) {
    let resolve = Rc::new(resolve);
    let last_y = Rc::new(std::cell::Cell::new(0.));
    let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    target.set_preload(true);
    let hint = {
        let resolve = resolve.clone();
        glib::clone!(
            #[weak]
            root,
            #[upgrade_or]
            None,
            move |target: &gtk::DropTarget, y: f64| {
                clear_row(&root);
                let (w, id) = resolve()?;
                if feedback_kind(target) != Some(false) || !available(&w, false) {
                    return None;
                }
                let position = w.gpu.borrow().as_ref()?.session.image_layer_drop_hint(
                    id,
                    (y / root.height().max(1) as f64).clamp(0., 1.) as f32,
                )?;
                root.add_css_class(match position {
                    LayerDropPosition::Above => "layer-drop-before",
                    LayerDropPosition::Below => "layer-drop-after",
                    LayerDropPosition::Into => "layer-drop-into",
                });
                Some(position)
            }
        )
    };
    let motion = glib::clone!(
        #[strong]
        last_y,
        #[strong]
        hint,
        move |target: &gtk::DropTarget, _: f64, y: f64| {
            last_y.set(y);
            if hint(target, y).is_some() {
                gdk::DragAction::COPY
            } else {
                gdk::DragAction::empty()
            }
        }
    );
    target.connect_enter(motion.clone());
    target.connect_motion(motion);
    target.connect_value_notify(glib::clone!(
        #[weak]
        root,
        move |target| {
            if let Some(drop) = target.current_drop() {
                let action = if hint(target, last_y.get()).is_some() {
                    gdk::DragAction::COPY
                } else {
                    gdk::DragAction::empty()
                };
                drop.status(action, action);
            } else {
                clear_row(&root);
            }
        }
    ));
    target.connect_leave(glib::clone!(
        #[weak]
        root,
        move |_| clear_row(&root)
    ));
    target.connect_drop(glib::clone!(
        #[weak]
        root,
        #[upgrade_or]
        false,
        move |_, value, _, y| {
            clear_row(&root);
            let Some((w, id)) = resolve() else {
                return false;
            };
            let Ok(files) = value.get::<gdk::FileList>() else {
                return false;
            };
            let position = w.gpu.borrow().as_ref().and_then(|g| {
                g.session.image_layer_drop_hint(
                    id,
                    (y / root.height().max(1) as f64).clamp(0., 1.) as f32,
                )
            });
            let Some(position) = position else {
                return false;
            };
            receive(
                &w,
                files.files(),
                None,
                Some(ImageLayerDestination {
                    target: layer_core::LayerId(id),
                    position,
                }),
            )
        }
    ));
    root.add_controller(target);
}
