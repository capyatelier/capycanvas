//! Serial application file activation, pinned to the receiving drawing window.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::{Rc, Weak},
    time::Duration,
};

struct Batch {
    files: Vec<gio::File>,
    target: Option<Weak<Workspace>>,
}
struct Launcher {
    batches: RefCell<VecDeque<Batch>>,
    running: Cell<bool>,
    windows: Weak<RefCell<Vec<Rc<Workspace>>>>,
}
fn active(app: &adw::Application, windows: &[Rc<Workspace>]) -> Option<Rc<Workspace>> {
    app.active_window()
        .and_then(|native| {
            windows
                .iter()
                .find(|w| w.window.upcast_ref::<gtk::Window>() == &native)
                .cloned()
        })
        .or_else(|| {
            windows
                .iter()
                .rev()
                .find(|w| w.window.is_visible())
                .cloned()
        })
}
pub(crate) fn install(app: &adw::Application, windows: &Rc<RefCell<Vec<Rc<Workspace>>>>) {
    let launcher = Rc::new(Launcher {
        batches: Default::default(),
        running: Cell::new(false),
        windows: Rc::downgrade(windows),
    });
    app.connect_open(glib::clone!(
        #[strong]
        launcher,
        move |app, files, _| {
            let target = launcher
                .windows
                .upgrade()
                .and_then(|v| active(app, &v.borrow()))
                .map(|w| Rc::downgrade(&w));
            launcher.batches.borrow_mut().push_back(Batch {
                files: files.to_vec(),
                target,
            });
            launcher.start(app);
        }
    ));
    let action = gio::SimpleAction::new(
        "open-in-window",
        Some(<(String, Vec<String>)>::static_variant_type().as_ref()),
    );
    action.connect_activate(glib::clone!(
        #[weak]
        app,
        #[strong]
        launcher,
        move |_, value| {
            let Some((name, uris)) = value.and_then(|v| v.get::<(String, Vec<String>)>()) else {
                return;
            };
            let target = launcher.windows.upgrade().and_then(|v| {
                v.borrow()
                    .iter()
                    .find(|w| w.window.widget_name() == name)
                    .cloned()
            });
            if let Some(target) = target {
                launcher.batches.borrow_mut().push_back(Batch {
                    files: uris.iter().map(|uri| gio::File::for_uri(uri)).collect(),
                    target: Some(Rc::downgrade(&target)),
                });
                launcher.start(&app);
            }
        }
    ));
    app.add_action(&action);
}
pub(crate) fn open_in(w: &Rc<Workspace>, files: Vec<gio::File>) {
    if let Some(app) = w.window.application() {
        let value = (
            w.window.widget_name().to_string(),
            files
                .iter()
                .map(|f| f.uri().to_string())
                .collect::<Vec<_>>(),
        )
            .to_variant();
        app.activate_action("open-in-window", Some(&value));
    }
}
impl Launcher {
    fn start(self: &Rc<Self>, app: &adw::Application) {
        if self.running.replace(true) {
            return;
        }
        let hold = app.hold();
        glib::spawn_future_local(glib::clone!(
            #[strong(rename_to=launcher)]
            self,
            #[weak]
            app,
            async move {
                let _hold = hold;
                loop {
                    let batch = launcher.batches.borrow_mut().pop_front();
                    let Some(batch) = batch else {
                        break;
                    };
                    launcher.run(&app, batch).await;
                }
                launcher.running.set(false);
            }
        ));
    }
    async fn run(&self, app: &adw::Application, batch: Batch) {
        let mut target = match batch.target {
            Some(target) => match target.upgrade().filter(|w| w.window.is_visible()) {
                Some(w) => Some(w),
                None => return,
            },
            None => self
                .windows
                .upgrade()
                .and_then(|v| active(app, &v.borrow())),
        };
        let closed = Rc::new(Cell::new(false));
        let placeholder = if target.is_none() {
            let window = adw::ApplicationWindow::builder()
                .application(app)
                .title("Open — Capy Canvas")
                .default_width(480)
                .default_height(300)
                .build();
            window.set_widget_name("file-launch-window");
            let view = adw::ToolbarView::new();
            view.add_top_bar(&adw::HeaderBar::new());
            view.set_content(Some(
                &adw::StatusPage::builder()
                    .title("Opening files")
                    .icon_name("image-x-generic-symbolic")
                    .build(),
            ));
            window.set_content(Some(&view));
            window.connect_close_request(glib::clone!(
                #[strong]
                closed,
                move |window| {
                    closed.set(true);
                    if let Some(dialog) = window.visible_dialog() {
                        dialog.force_close();
                    }
                    window.set_visible(false);
                    glib::Propagation::Stop
                }
            ));
            window.present();
            Some(window)
        } else {
            None
        };
        for file in batch.files {
            if closed.get() {
                break;
            }
            if let Some(w) = &target {
                while w.window.is_visible()
                    && (w.servicing.get()
                        || w.documents.changing.get()
                        || w.window.visible_dialog().is_some())
                {
                    glib::timeout_future(Duration::from_millis(20)).await;
                }
                if !w.window.is_visible() || w.documents.closing_window.get() {
                    break;
                }
                w.documents.loading.set(true);
                w.documents.cancel_open.set(false);
            }
            let parent = target
                .as_ref()
                .map(|w| &w.window)
                .or(placeholder.as_ref())
                .unwrap();
            parent.present();
            let settings = target
                .as_ref()
                .and_then(|w| {
                    w.gpu
                        .borrow()
                        .as_ref()
                        .map(|g| g.session.state().settings.clone())
                })
                .unwrap_or_else(|| {
                    crate::preferences::load()
                        .ok()
                        .flatten()
                        .unwrap_or_default()
                });
            let name = file
                .basename()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.uri().into());
            let result = super::open::prepare(
                parent,
                file,
                settings.photo_open,
                settings.new_document.defaults.color.space,
            )
            .await;
            if let Some(w) = &target {
                w.documents.loading.set(false);
            }
            if closed.get()
                || !parent.is_visible()
                || target
                    .as_ref()
                    .is_some_and(|w| w.documents.cancel_open.get())
            {
                break;
            }
            match result {
                Ok(Some(project)) => {
                    if let Some(w) = &target {
                        if let Err(error) = w.documents.open(w, (project.0, project.1, None)).await
                        {
                            w.changed(Err(error));
                            break;
                        }
                    } else if let Some(windows) = self.windows.upgrade() {
                        crate::open_workspace(app, &windows, Some(project), None);
                        target = windows.borrow().last().cloned();
                        if let Some(window) = &placeholder {
                            window.destroy();
                        }
                        if let Some(w) = &target {
                            crate::recovery::offer_stale(w);
                        }
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let dialog = adw::AlertDialog::builder()
                        .heading("Cannot open file")
                        .body(format!("{name}\n\n{error}"))
                        .build();
                    dialog.set_widget_name("file-launch-error");
                    dialog.add_response("ok", "OK");
                    dialog.set_close_response("ok");
                    crate::alert::choose(dialog, parent).await;
                }
            }
        }
        if let Some(window) = placeholder {
            window.destroy();
        }
        if let Some(w) = target {
            w.documents.loading.set(false);
            if w.documents.cancel_open.get() {
                w.window.close();
            }
        }
    }
}
