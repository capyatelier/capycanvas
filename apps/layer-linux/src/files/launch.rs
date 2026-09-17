//! Application Open transport. Decode before creating a canvas, serializing
//! file-list delivery (including secondary-process launches) on one worker.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::{Rc, Weak},
};

struct Launcher {
    files: RefCell<VecDeque<gio::File>>,
    running: Cell<bool>,
    windows: Weak<RefCell<Vec<Rc<Workspace>>>>,
}

pub(crate) fn install(app: &adw::Application, windows: &Rc<RefCell<Vec<Rc<Workspace>>>>) {
    let launcher = Rc::new(Launcher {
        files: RefCell::new(VecDeque::new()),
        running: Cell::new(false),
        windows: Rc::downgrade(windows),
    });
    app.connect_open(move |app, files, _| {
        launcher.files.borrow_mut().extend(files.iter().cloned());
        launcher.start(app);
    });
}

impl Launcher {
    fn start(self: &Rc<Self>, app: &adw::Application) {
        if self.files.borrow().is_empty() || self.running.replace(true) {
            return;
        }
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Open — Capy Canvas")
            .default_width(480)
            .default_height(300)
            .build();
        window.set_widget_name("file-launch-window");
        let content = adw::ToolbarView::new();
        content.add_top_bar(&adw::HeaderBar::new());
        let page = adw::StatusPage::builder()
            .title("Opening files")
            .icon_name("image-x-generic-symbolic")
            .build();
        content.set_content(Some(&page));
        window.set_content(Some(&content));
        let closed = Rc::new(Cell::new(false));
        window.connect_close_request(glib::clone!(
            #[weak(rename_to = launcher)]
            self,
            #[strong]
            closed,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |window| {
                closed.set(true);
                launcher.files.borrow_mut().clear();
                if let Some(dialog) = window.visible_dialog() {
                    dialog.force_close();
                }
                window.set_visible(false);
                // Keep the application and parent alive until the reader has
                // acknowledged cancellation. No late result may create a canvas.
                glib::Propagation::Stop
            }
        ));
        window.present();
        let hold = app.hold();
        let app = app.clone();
        let launcher = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let _hold = hold;
            loop {
                if closed.get() {
                    break;
                }
                let Some(file) = launcher.files.borrow_mut().pop_front() else {
                    break;
                };
                let name = file
                    .basename()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file.uri().into());
                page.set_description(Some(&name));
                window.present();
                let settings = launcher.windows.upgrade().and_then(|windows| {
                    windows.borrow().last().and_then(|w| {
                        w.gpu
                            .borrow()
                            .as_ref()
                            .map(|g| g.session.state().settings.clone())
                    })
                });
                let settings = match settings {
                    Some(settings) => settings,
                    None => gio::spawn_blocking(crate::preferences::load)
                        .await
                        .unwrap_or_else(|_| Err("Preferences reader failed".into()))
                        .unwrap_or_else(|error| {
                            eprintln!("{error}; using default preferences");
                            None
                        })
                        .unwrap_or_default(),
                };
                if closed.get() {
                    break;
                }
                let result = super::open::prepare(
                    &window,
                    file,
                    settings.photo_open,
                    settings.new_document.defaults.color.space,
                )
                .await;
                if closed.get() {
                    break;
                }
                match result {
                    Ok(Some(project)) => {
                        if let Some(windows) = launcher.windows.upgrade() {
                            let first = windows.borrow().is_empty();
                            crate::open_workspace(&app, &windows, Some(project), None);
                            if first {
                                let workspace = windows.borrow().last().cloned();
                                if let Some(workspace) = workspace {
                                    crate::recovery::offer_stale(&workspace);
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        launcher.files.borrow_mut().clear();
                        break;
                    }
                    Err(error) => {
                        let dialog = adw::AlertDialog::builder()
                            .heading("Cannot open file")
                            .body(format!("{name}\n\n{error}"))
                            .build();
                        dialog.set_widget_name("file-launch-error");
                        dialog.add_response("ok", "OK");
                        dialog.set_close_response("ok");
                        crate::alert::choose(dialog, &window).await;
                    }
                }
            }
            window.destroy();
            launcher.running.set(false);
            // A new application Open received while the previous worker was
            // acknowledging window-close cancellation starts its own batch.
            launcher.start(&app);
        });
    }
}
