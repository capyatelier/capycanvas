//! Native file-list receipt. Capture document identity, destination and the
//! camera-derived point before file preparation; shared Rust owns placement.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use layer_ui::{FileDropRefusal, ImageLayerDestination, ImportIntent, LayerDropPosition};
use std::rc::Rc;

pub(crate) struct Incoming {
    pub request: u32,
    pub files: Vec<gio::File>,
    pub center: Option<layer_core::Point>,
    pub destination: Option<ImageLayerDestination>,
}
fn intent(files: &[gio::File], onto_layer: bool) -> Result<ImportIntent, FileDropRefusal> {
    ImportIntent::for_drop(files.iter().map(|file| file.path().is_some_and(|path| ImportIntent::is_drawing_path(&path))), onto_layer)
}
fn available(w: &Workspace, intent: ImportIntent) -> bool {
    let gpu = w.gpu.borrow();
    let Some(g) = gpu.as_ref() else { return false; };
    let mut incoming = w.image_drop.borrow_mut();
    if incoming.as_ref().is_some_and(|drop| !g.session.file_drop_pending(drop.request)) { incoming.take(); }
    incoming.is_none() && g.session.command(intent.command()).enabled
}
fn files(target: &gtk::DropTarget) -> Option<Vec<gio::File>> {
    Some(target.value()?.get::<gdk::FileList>().ok()?.files())
}
fn feedback_intent(target: &gtk::DropTarget) -> Option<ImportIntent> {
    files(target).map_or(Some(ImportIntent::Place), |files| intent(&files, true).ok())
}
pub(crate) fn receive(
    w: &Rc<Workspace>,
    files: Vec<gio::File>,
    center: Option<layer_core::Point>,
    destination: Option<ImageLayerDestination>,
) -> bool {
    let result = (|| {
        let intent = intent(&files, destination.is_some()).map_err(|refusal| refusal.message(&w.localization()).to_string())?;
        if !available(w, intent) {
            return Err("Finish the current operation before dropping files".into());
        }
        let mut gpu = w.gpu.borrow_mut();
        let g = gpu.as_mut().ok_or("Canvas unavailable")?;
        let (request, change) = g.session.request_file_drop(intent)?;
        *w.image_drop.borrow_mut() = Some(Incoming { request, files, center, destination });
        Ok(change)
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
            let intent = files(target).map_or(Ok(ImportIntent::Place), |files| intent(&files, false));
            let valid = intent.is_ok_and(|intent| available(&w, intent));
            let label: std::sync::Arc<str> = match intent {
                Ok(ImportIntent::Open) => "Open drawing".into(),
                Ok(ImportIntent::Place) => "Add images as layers".into(),
                Err(refusal) => refusal.message(&w.localization()),
            };
            w.image_drop_label.set_label(&label);
            // A forbidden cursor alone cannot explain an unsupported batch.
            // Keep its reason visible while the pointer is over the canvas.
            w.image_drop_label.set_visible(valid || intent.is_err());
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
            let scale = w.area.scale_factor() as f32;
            let center = layer_core::Point { x: x as f32 * scale, y: y as f32 * scale };
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
                if feedback_intent(target) != Some(ImportIntent::Place) || !available(&w, ImportIntent::Place) {
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
                    LayerDropPosition::Attach => return None,
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
            let Ok(target) = layer_ui::occurrence_handle(id) else { return false; };
            receive(
                &w,
                files.files(),
                None,
                Some(ImageLayerDestination {
                    target,
                    position,
                }),
            )
        }
    ));
    root.add_controller(target);
}
