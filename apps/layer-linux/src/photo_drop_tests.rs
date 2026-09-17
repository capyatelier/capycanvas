//! File-manager-style URI delivery from another process through real Wayland DND.
//! Run in the isolated compositor; no callback injection into production targets.
use super::new_photo::{finish, invoke, ready};
use super::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[path = "photo_workflow_tests.rs"]
mod workflow;

fn publish(path: &Path, value: &Value) {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(value).unwrap()).unwrap();
    std::fs::rename(temporary, path).unwrap();
}
fn read(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}
fn until(mut predicate: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(Instant::now() < deadline, "{message}");
        pump(10);
    }
}

/// This helper is launched by the receiving test. It has its own GDK display
/// connection, and offers only text/uri-list, forcing native MIME negotiation.
#[test]
#[ignore = "child process of native_photo_file_drops"]
fn native_photo_file_drag_source() {
    assert_eq!(std::env::var("LAYER_PHOTO_DROP_HELPER").as_deref(), Ok("1"));
    let dir = PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.PhotoDragSource");
    let label = gtk::Label::new(Some("Drag image files from this window"));
    let window = gtk::ApplicationWindow::builder()
        .application(&*app)
        .title("Photo file drag source")
        .decorated(false)
        .child(&label)
        .build();
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::COPY);
    source.connect_prepare(glib::clone!(
        #[strong]
        dir,
        move |_, _, _| {
            let files = read(&dir.join("source-files.json"))?;
            let uris = files
                .as_array()?
                .iter()
                .map(|path| {
                    format!(
                        "{}\r\n",
                        gtk::gio::File::for_path(path.as_str().unwrap()).uri()
                    )
                })
                .collect::<String>();
            Some(gdk::ContentProvider::for_bytes(
                "text/uri-list",
                &glib::Bytes::from_owned(uris),
            ))
        }
    ));
    let begins = Rc::new(Cell::new(0));
    source.connect_drag_begin(glib::clone!(
        #[strong]
        begins,
        #[strong]
        dir,
        move |_, drag| {
            begins.set(begins.get() + 1);
            publish(
                &dir.join("source-begin.json"),
                &json!({
                    "count": begins.get(), "device": format!("{:?}", drag.device().source()),
                }),
            );
        }
    ));
    label.add_controller(source);
    window.maximize();
    window.present();
    let deadline = Instant::now() + Duration::from_secs(180);
    while !dir.join("source-stop").exists() {
        assert!(Instant::now() < deadline, "receiving test did not finish");
        pump(20);
        publish(
            &dir.join("source-window.json"),
            &json!({
                "width": window.width(), "height": window.height(), "active": window.is_active(),
            }),
        );
    }
    window.destroy();
}

struct FileDrag {
    dir: PathBuf,
    child: std::process::Child,
    step: usize,
    begins: u64,
    start: [f32; 2],
}
impl Drop for FileDrag {
    fn drop(&mut self) {
        let _ = std::fs::write(self.dir.join("source-stop"), b"stop");
        // Reap this test's own child even during an assertion failure.
        pump(100);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl FileDrag {
    fn stop_source(&mut self) {
        std::fs::write(self.dir.join("source-stop"), b"stop").unwrap();
        until(|| self.child.try_wait().unwrap().is_some(), "source shutdown");
        pump(200);
    }
    fn start(w: &Workspace) -> Self {
        let dir = PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap());
        std::fs::write(dir.join("ready"), b"ready").unwrap();
        pump(500);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg(format!(
                "{}::native_photo_file_drag_source",
                module_path!().split_once("::").unwrap().1
            ))
            .args(["--exact", "--ignored", "--nocapture"])
            .env("LAYER_PHOTO_DROP_HELPER", "1")
            .spawn()
            .unwrap();
        let mut driver = Self {
            dir,
            child,
            step: 0,
            begins: 0,
            start: [0.; 2],
        };
        until(
            || read(&driver.dir.join("source-window.json")).is_some_and(|v| v["active"] == true),
            "source activation",
        );
        // Mutter's standard Super+Left tiles the source to the left half of the
        // private display. The maximized editor stays visible on the right.
        driver.events(json!([
            {"key": 0xffeb, "down": true}, {"key": 0xff51, "down": true},
            {"key": 0xff51, "down": false}, {"key": 0xffeb, "down": false}
        ]));
        let width = w.window.width();
        until(
            || {
                read(&driver.dir.join("source-window.json"))
                    .is_some_and(|v| v["width"].as_i64() == Some(i64::from(width / 2)))
            },
            "source must tile left",
        );
        driver.start = [width as f32 * 0.25, w.window.height() as f32 * 0.5];
        driver
    }
    fn events(&mut self, events: Value) {
        publish(&self.dir.join(format!("step-{}.json", self.step)), &events);
        until(
            || self.dir.join(format!("done-{}", self.step)).exists(),
            "native input acknowledgement",
        );
        self.step += 1;
        pump(150);
    }
    fn hover(&mut self, paths: &[PathBuf], point: [f32; 2], touch: bool) {
        publish(&self.dir.join("source-files.json"), &json!(paths));
        if read(&self.dir.join("source-window.json")).unwrap()["active"] != true {
            self.events(json!([
                {"key": 0xffe9, "down": true}, {"key": 0xff09, "down": true},
                {"key": 0xff09, "down": false}, {"key": 0xffe9, "down": false}
            ]));
        }
        until(
            || read(&self.dir.join("source-window.json")).is_some_and(|v| v["active"] == true),
            "source focus",
        );
        let slop = gtk::Settings::default().unwrap().gtk_dnd_drag_threshold() as f32;
        let pickup = [self.start[0] + slop * 3., self.start[1]];
        let armed = [pickup[0] + slop * 3., pickup[1]];
        self.events(if touch {
            json!([
                {"touch": "down", "point": self.start}, {"wait_ms": 80},
                {"touch": "move", "point": pickup}, {"wait_ms": 80},
                {"touch": "move", "point": armed}
            ])
        } else {
            json!([
                {"point": self.start}, {"wait_ms": 80}, {"down": true}, {"wait_ms": 80},
                {"point": pickup}, {"wait_ms": 80}, {"point": armed}
            ])
        });
        self.begins += 1;
        until(
            || read(&self.dir.join("source-begin.json")).is_some_and(|v| v["count"] == self.begins),
            "native source pickup before crossing windows",
        );
        let begun = read(&self.dir.join("source-begin.json")).unwrap();
        assert_eq!(begun["count"], self.begins);
        assert_eq!(begun["device"], if touch { "Touchscreen" } else { "Mouse" });
        let approach = [point[0] - 4., point[1]];
        self.events(if touch {
            json!([
                {"touch": "move", "point": approach}, {"touch": "move", "point": point}
            ])
        } else {
            json!([{"point": approach}, {"point": point}])
        });
    }
    fn release(&mut self, touch: bool) {
        self.events(if touch {
            json!([{"touch": "up"}])
        } else {
            json!([{"down": false}])
        });
    }
    fn click_placement(&mut self, w: &Workspace, name: &str) {
        let button = find_named(w.window.upcast_ref(), name).expect("visible placement action");
        assert!(button.is_mapped() && button.is_sensitive(), "{name}");
        let bounds = button.compute_bounds(&w.window).unwrap();
        let point = [
            bounds.x() + bounds.width() * 0.5,
            bounds.y() + bounds.height() * 0.5,
        ];
        self.events(json!([{"point": point}, {"down": true}, {"down": false}]));
        if name != "placement-original-size" {
            until(
                || !w.placement_actions.root.is_visible(),
                "placement controls retire after native click",
            );
        }
    }
}

#[test]
#[ignore = "isolated compositor and native keyboard delivery"]
#[allow(deprecated)]
fn native_multiple_photo_import_chooser() {
    fn file_list(root: &gtk::Widget) -> Option<gtk::Widget> {
        if root.is_mapped() && (root.is::<gtk::ColumnView>() || root.is::<gtk::TreeView>()) {
            return Some(root.clone());
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Some(found) = file_list(&widget) { return Some(found); }
            child = widget.next_sibling();
        }
        None
    }
    let app = native_test_app("art.capycanvas.MultiplePhotoImport");
    let w = Workspace::with_project(&app, Some((new_drawing(200, 150).unwrap(), None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let mut driver = FileDrag::start(&w);
    driver.stop_source();
    w.window.present();
    let directory = driver.dir.join("chooser-photos");
    std::fs::create_dir(&directory).unwrap();
    let source = super::place_source::source();
    let paths = [directory.join("First photo.png"), directory.join("Second photo.png")];
    for path in &paths {
        layer_color::photo::write_png(std::fs::File::create(path).unwrap(), &source).unwrap();
    }
    let before = w.gpu.borrow().as_ref().unwrap().session.engine().document().layers.clone();
    for apply in [false, true] {
        invoke(&w, CommandId::ImportImage);
        let chooser = super::new_photo::chooser();
        assert!(chooser.selects_multiple());
        chooser.set_file(&gtk::gio::File::for_path(&paths[0])).unwrap();
        pump(300);
        let list = file_list(chooser.upcast_ref()).expect("visible native chooser file list");
        assert!(list.grab_focus());
        // Select and accept through Mutter's virtual keyboard and Wayland.
        // No callback supplies a synthetic list of selected files to the app.
        driver.events(json!([
            {"key": 0xffe3, "down": true}, {"key": 0x61, "down": true},
            {"key": 0x61, "down": false}, {"key": 0xffe3, "down": false}
        ]));
        until(|| chooser.files().n_items() == 2, "native chooser selects both files");
        let selected: Vec<_> = chooser.files().iter::<gtk::gio::File>()
            .map(|file| file.unwrap().path().unwrap()).collect();
        assert_eq!(selected, paths);
        driver.events(json!([{"key": 0xff0d, "down": true}, {"key": 0xff0d, "down": false}]));
        until(|| !chooser.is_visible(), "native Return accepts the chooser");
        finish(&w);
        ready(&w);
        let imported = w.gpu.borrow().as_ref().unwrap().session.engine().document().layers.clone();
        let photos: Vec<_> = imported.iter().filter(|layer| layer.source.is_some()).collect();
        assert_eq!(imported.len(), before.len() + 2);
        assert_eq!(photos.iter().map(|layer| layer.name.as_ref()).collect::<Vec<_>>(),
            ["First photo", "Second photo"]);
        for layer in photos {
            assert_eq!(layer.source.as_deref(), Some(&source));
            assert!(layer.raster.is_empty());
        }
        driver.click_placement(&w, if apply { "placement-apply" } else { "placement-cancel" });
        ready(&w);
        if apply {
            let saved = super::place_source::snapshot(&w);
            let reopened = layer_core::Project::read(saved.as_slice(), Default::default()).unwrap();
            assert_eq!(reopened.document.layers, imported);
            invoke(&w, CommandId::Undo);
            ready(&w);
        }
        assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().document().layers, before);
    }
    println!("native chooser multiple selection: ordered retained sources, Cancel, Apply, save/reopen and one Undo passed");
    std::fs::write(driver.dir.join("finished"), b"finished").unwrap();
    w.window.destroy();
}

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_photo_file_drops"]
fn native_photo_file_drops() {
    let app = native_test_app("art.capycanvas.PhotoFileDrops");
    let mut project = new_drawing(200, 150).unwrap();
    let group = project.document.allocate_layer_id();
    let mut row = layer_core::Layer::paint(group, "Photo destination");
    row.kind = layer_core::LayerKind::Group;
    row.properties.offset = Point { x: 40., y: -10. };
    project.document.layers[0].properties.parent = Some(group);
    project.document.layers.insert(0, row);
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(layer_ui::WorkspaceState::default()),
    });
    pump(300);
    invoke(&w, CommandId::RotateRight);
    invoke(&w, CommandId::ZoomOut);
    ready(&w);
    let dir = PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap());
    let original = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .layers
        .clone();
    let mut builder = layer_core::color::source::SourceBuilder::new(
        [1200, 800],
        super::place_source::source().interpretation,
        16 << 20,
    )
    .unwrap();
    for y in 0..800 {
        let row: Vec<u8> = (0..1200)
            .flat_map(|x| [65535u16, (x * 47) as u16, (y * 71) as u16, 65535])
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    let source = builder.finish().unwrap();
    let paths = [dir.join("First photo.png"), dir.join("Second – photo.png")];
    for path in &paths {
        layer_color::photo::write_png(std::fs::File::create(path).unwrap(), &source).unwrap();
    }
    let mut driver = FileDrag::start(&w);
    let area = w.area.compute_bounds(&w.window).unwrap();
    let point = ((w.window.width() / 2 + 40)..(w.window.width() - 40))
        .step_by(20)
        .map(|x| [x as f32, area.y() + area.height() * 0.52])
        .find(|p| {
            w.window
                .pick(p[0] as f64, p[1] as f64, gtk::PickFlags::DEFAULT)
                .is_some_and(|picked| picked == w.area)
        })
        .expect("uncovered canvas to the right of the source window");
    println!(
        "canvas drop point {point:?}, area {area:?}, viewport {:?}",
        state(&w).camera.viewport
    );
    let scale = w.area.scale_factor() as f32;
    let center = state(&w).camera.input_transform().map(Point {
        x: (point[0] - area.x()) * scale,
        y: (point[1] - area.y()) * scale,
    });
    for touch in [false, true] {
        driver.hover(&paths, point, touch);
        assert!(
            w.image_drop_label.is_visible(),
            "native canvas COPY preview"
        );
        driver.release(touch);
        finish(&w);
        ready(&w);
        assert!(!w.image_drop_label.is_visible());
        let doc = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .clone();
        assert_eq!(doc.layers.len(), original.len() + 2);
        let photos: Vec<_> = doc.layers.iter().filter(|l| l.source.is_some()).collect();
        assert_eq!(
            photos.iter().map(|l| l.name.as_ref()).collect::<Vec<_>>(),
            ["First photo", "Second – photo"]
        );
        for photo in photos {
            assert_eq!(photo.source.as_deref(), Some(&source));
            let actual = doc
                .layer_transform(photo.id)
                .map(Point { x: 600., y: 400. });
            assert!(
                (actual.x - center.x).abs() < 0.5 && (actual.y - center.y).abs() < 0.5,
                "drop camera mapping {actual:?} != {center:?}"
            );
            assert!(photo.raster.is_empty());
        }
        assert!(
            state(&w)
                .commands
                .iter()
                .any(|c| c.id == CommandId::ApplyTransform && c.enabled)
        );
        if !touch {
            let report =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/image-placement");
            std::fs::create_dir_all(&report).unwrap();
            super::new_photo::capture_ui(&w, &report, "native-photo-batch.png");
        }
        driver.click_placement(&w, "placement-apply");
        ready(&w);
        let saved = super::place_source::snapshot(&w);
        let reopened = layer_core::Project::read(saved.as_slice(), Default::default()).unwrap();
        assert_eq!(reopened.document.layers, doc.layers);
        invoke(&w, CommandId::Undo);
        ready(&w);
        assert_eq!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .document()
                .layers,
            original
        );
        println!(
            "native external {} canvas batch: source profiles, camera mapping, Apply/save/one undo passed",
            if touch { "touch" } else { "mouse" }
        );
    }
    for (fraction, class, parent) in [
        (0.1, "layer-drop-before", None),
        (0.5, "layer-drop-into", Some(group)),
        (0.9, "layer-drop-after", None),
    ] {
        let row = find_named(
            w.layer_panel.root.upcast_ref(),
            &format!("art-layer-{}", group.0),
        )
        .unwrap();
        let bounds = row.compute_bounds(&w.window).unwrap();
        let target = [
            bounds.x() + bounds.width() * 0.65,
            bounds.y() + bounds.height() * fraction,
        ];
        driver.hover(&paths[..1], target, false);
        assert!(row.has_css_class(class), "native row drop marker {class}");
        driver.release(false);
        finish(&w);
        ready(&w);
        assert!(!row.has_css_class(class));
        let doc = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .clone();
        let photo = doc.layer(doc.active_layer).unwrap();
        assert_eq!(photo.properties.parent, parent);
        assert_eq!(
            doc.layer_transform(photo.id)
                .map(Point { x: 600., y: 400. }),
            Point { x: 100., y: 75. }
        );
        let expected = if fraction < 0.2 {
            0
        } else if fraction < 0.8 {
            1
        } else {
            2
        };
        assert_eq!(
            doc.layers.iter().position(|l| l.id == photo.id),
            Some(expected)
        );
        driver.click_placement(&w, "placement-cancel");
        ready(&w);
        assert_eq!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .document()
                .layers,
            original
        );
        println!("native external row {class}: destination, group offset and Cancel passed");
    }
    // The first file decodes successfully; failure in the second must publish
    // neither, clear the reservation, and leave no active placement/history.
    let broken = dir.join("Broken photo.png");
    std::fs::write(&broken, b"not a valid photo").unwrap();
    driver.hover(&[paths[0].clone(), broken], point, false);
    driver.release(false);
    until(
        || !state(&w).document_file.busy && state(&w).requests.is_empty() && !w.servicing.get(),
        "failed batch retires worker",
    );
    assert!(
        state(&w)
            .host_error
            .as_deref()
            .is_some_and(|e| e.contains("Broken photo") && e.contains("No images were imported"))
    );
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers,
        original
    );
    assert!(
        !state(&w)
            .commands
            .iter()
            .any(|c| c.id == CommandId::ApplyTransform && c.enabled)
    );

    // Cancel through the actual native progress UI before owner publication.
    let cancellations = Rc::new(Cell::new(0));
    let signal = w.window.connect_notify_local(
        Some("visible-dialog"),
        glib::clone!(
            #[strong]
            cancellations,
            move |window, _| {
                let Some(dialog) = window
                    .visible_dialog()
                    .filter(|d| d.widget_name() == "image-import-progress")
                else {
                    return;
                };
                let cancellations = cancellations.clone();
                glib::idle_add_local_full(glib::Priority::HIGH, move || {
                    cancellations.set(cancellations.get() + 1);
                    find_button(dialog.upcast_ref(), "Cancel")
                        .unwrap()
                        .emit_clicked();
                    glib::ControlFlow::Break
                });
            }
        ),
    );
    driver.hover(&paths, point, false);
    driver.release(false);
    finish(&w);
    assert_eq!(cancellations.get(), 1);
    w.window.disconnect(signal);
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers,
        original
    );
    assert!(!w.servicing.get(), "Cancel waits for the worker to retire");

    // A queued owner edit can change the destination while the worker runs.
    // Reject its stale result without undoing that independent selection.
    let signal = w.window.connect_notify_local(
        Some("visible-dialog"),
        glib::clone!(
            #[weak]
            w,
            move |window, _| {
                if window
                    .visible_dialog()
                    .is_some_and(|d| d.widget_name() == "image-import-progress")
                {
                    glib::idle_add_local_full(
                        glib::Priority::HIGH,
                        glib::clone!(
                            #[weak]
                            w,
                            #[upgrade_or]
                            glib::ControlFlow::Break,
                            move || {
                                w.dispatch(UiAction::Layer {
                                    action: layer_ui::LayerAction::Select {
                                        id: group.0,
                                        mask: false,
                                    },
                                });
                                glib::ControlFlow::Break
                            }
                        ),
                    );
                }
            }
        ),
    );
    driver.hover(&paths, point, false);
    driver.release(false);
    until(
        || !state(&w).document_file.busy && state(&w).requests.is_empty() && !w.servicing.get(),
        "stale batch retires worker",
    );
    w.window.disconnect(signal);
    assert!(
        state(&w)
            .host_error
            .as_deref()
            .is_some_and(|e| e.contains("changed while importing")),
        "stale destination: {:?}",
        state(&w).host_error
    );
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .active_layer,
        group
    );
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers,
        original
    );

    // Project/image mixtures get an explanatory forbidden-drop preview. A
    // single project follows Open and never becomes a layer in this document.
    let native = dir.join("Drawing.capy");
    std::fs::write(&native, super::place_source::snapshot(&w)).unwrap();
    driver.hover(&[native.clone(), paths[0].clone()], point, false);
    assert!(w.image_drop_label.is_visible());
    assert!(
        w.image_drop_label
            .label()
            .contains("batch containing only images")
    );
    driver.release(false);
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers,
        original
    );
    let opened = Rc::new(RefCell::new(None));
    *w.open_document.borrow_mut() = Some(Rc::new(glib::clone!(
        #[strong]
        opened,
        move |project, location, _| {
            opened.replace(Some((project, location)));
        }
    )));
    driver.hover(&[native], point, false);
    assert_eq!(w.image_drop_label.label(), "Open drawing");
    driver.release(false);
    finish(&w);
    let (project, location) = opened
        .borrow_mut()
        .take()
        .expect("native project drop invokes Open");
    assert_eq!(project.document.layers, original);
    assert!(location.is_some());
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers,
        original
    );
    println!(
        "native external batches: malformed second file, Cancel, stale target, mixed-batch feedback and project Open passed"
    );
    std::fs::write(driver.dir.join("finished"), b"finished").unwrap();
    drop(driver);
    w.window.destroy();
}
