//! Real GApplication argument forwarding on the private session bus. The sender
//! runs production application setup; the receiver inspects native documents.
use super::*;
use gtk::gio;
use layer_core::color::{ColorProfile, SampleDepth, source::*};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

fn until(mut ready: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "{message}");
        pump(10);
    }
}

#[test]
#[ignore = "secondary process for native_application_file_launch"]
fn application_file_launch_sender() {
    let Ok(id) = std::env::var("LAYER_FILE_LAUNCH_APP_ID") else {
        return;
    };
    let files: Vec<String> =
        serde_json::from_str(&std::env::var("LAYER_FILE_LAUNCH_ARGS").unwrap()).unwrap();
    let (app, _) = crate::application(&id);
    let args = std::iter::once("capycanvas".to_owned())
        .chain(files)
        .collect::<Vec<_>>();
    assert_eq!(app.run_with_args(&args), glib::ExitCode::SUCCESS);
}

struct Sender(Child);
impl Sender {
    fn new(id: &str, files: &[&Path]) -> Self {
        let files: Vec<_> = files.iter().map(|p| p.to_str().unwrap()).collect();
        Self(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "application_file_launch_sender",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("LAYER_FILE_LAUNCH_APP_ID", id)
                .env(
                    "LAYER_FILE_LAUNCH_ARGS",
                    serde_json::to_string(&files).unwrap(),
                )
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn finish(&mut self) {
        until(
            || self.0.try_wait().unwrap().is_some(),
            "secondary application completed",
        );
        assert!(self.0.wait().unwrap().success());
    }
}
impl Drop for Sender {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn launch_window(app: &adw::Application) -> Option<adw::ApplicationWindow> {
    app.windows()
        .into_iter()
        .find(|w| w.widget_name() == "file-launch-window")
        .and_then(|w| w.downcast().ok())
}

#[test]
#[ignore = "isolated Wayland/session bus and hardware GPU"]
fn native_application_file_launch() {
    adw::init().unwrap();
    let id = format!("art.capycanvas.FileLaunch.p{}", std::process::id());
    let (app, windows) = crate::application(&id);
    let app = NativeTestApp(app);
    app.register(None::<&gio::Cancellable>).unwrap();
    assert!(!app.is_remote());
    assert!(windows.borrow().is_empty() && app.windows().is_empty());
    let settings_path =
        PathBuf::from(std::env::var_os("LAYER_SETTINGS_FILE").expect("isolated settings"));
    assert!(settings_path.starts_with(std::env::temp_dir()));
    let mut settings = layer_ui::Settings::default();
    settings.photo_open.promote_to_16 = true;
    settings.photo_open.missing_profile = layer_ui::MissingProfilePolicy::Ask;
    std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let activated = Rc::new(Cell::new(false));
    app.connect_window_added(glib::clone!(
        #[strong]
        activated,
        move |app, window| {
            let application = app.downgrade();
            let activated = activated.clone();
            window.connect_map(move |window| {
                if window.widget_name() == "file-launch-window" && !activated.replace(true) {
                    application.upgrade().unwrap().activate();
                }
            });
        }
    ));
    let directory = PathBuf::from(std::env::var_os("LAYER_FILE_LAUNCH_OUTPUT").unwrap_or_else(
        || {
            std::env::temp_dir()
                .join(format!("capy-file-launch-{}", std::process::id()))
                .into()
        },
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let photo = directory.join("Photograph with spaces.jpg");
    let mut source = SourceBuilder::new(
        [96, 64],
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: SampleDepth::U8,
            profile: ColorProfile::default(),
            profile_assumed: false,
        },
        1 << 20,
    )
    .unwrap();
    for y in 0..64 {
        source
            .push_row(
                &(0..96)
                    .flat_map(|x| [x as u8 * 2, y as u8 * 3, 155])
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    layer_color::photo::write_jpeg(
        std::fs::File::create(&photo).unwrap(),
        &source.finish().unwrap(),
        95,
    )
    .unwrap();
    let expected = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(&photo).unwrap()),
        Default::default(),
    )
    .unwrap();
    let master = directory.join("Master drawing.capy");
    let project = new_drawing(321, 217).unwrap();
    project
        .write(std::fs::File::create(&master).unwrap())
        .unwrap();
    let photo_before = std::fs::read(&photo).unwrap();
    let master_before = std::fs::read(&master).unwrap();

    // Cold multi-file launch creates one drawing window and no blank tab.
    let mut sender = Sender::new(&id, &[&photo, &master]);
    until(
        || {
            windows.borrow().first().is_some_and(|w| {
                w.documents.len() == 2 && !w.documents.changing.get() && !w.documents.loading.get()
            }) && launch_window(&app).is_none()
        },
        "cold launch creates tabs",
    );
    sender.finish();
    let w = windows.borrow()[0].clone();
    new_photo::ready(&w);
    assert_eq!(app.windows().len(), 1);
    assert_eq!(windows.borrow().len(), 1);
    assert!(activated.get());
    let second = w.documents.selected();
    assert_eq!(
        w.gpu.borrow().as_ref().unwrap().session.engine().document(),
        &project.document
    );
    assert_eq!(
        state(&w).document_file.location.unwrap().uri,
        gio::File::for_path(&master).uri()
    );
    glib::MainContext::default()
        .block_on(w.documents.activate(&w, 1))
        .unwrap();
    new_photo::ready(&w);
    {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().unwrap().session;
        let doc = session.engine().document();
        assert_eq!([doc.width, doc.height], expected.extent);
        assert_eq!(doc.color.depth, SampleDepth::U16);
        assert_eq!(doc.layers[0].source.as_deref(), Some(&expected));
        assert!(session.state().document_file.location.is_none());
    }
    new_photo::invoke(&w, CommandId::AddLayer);
    let edited = place_source::snapshot(&w);
    let mut sender = Sender::new(&id, &[&photo]);
    until(
        || w.documents.len() == 3 && !w.documents.changing.get() && !w.documents.loading.get(),
        "warm file launch",
    );
    sender.finish();
    assert_eq!(app.windows().len(), 1);
    glib::MainContext::default()
        .block_on(w.documents.activate(&w, 1))
        .unwrap();
    new_photo::ready(&w);
    assert_eq!(place_source::snapshot(&w), edited);
    // Error acknowledgement continues a batch in the receiving window.
    let bad = directory.join("Broken picture.jpg");
    std::fs::write(&bad, b"not an image").unwrap();
    let mut sender = Sender::new(&id, &[&bad, &master]);
    until(
        || {
            w.window
                .visible_dialog()
                .is_some_and(|d| d.widget_name() == "file-launch-error")
        },
        "failed file explanation",
    );
    assert_eq!(app.windows().len(), 1);
    new_photo::response(&w, "ok");
    until(
        || w.documents.len() == 4 && !w.documents.changing.get() && !w.documents.loading.get(),
        "batch continues",
    );
    sender.finish();
    // A second explicit window remains possible; its opens target that window.
    app.activate_action("new-window", None);
    until(|| windows.borrow().len() == 2, "explicit new window");
    let other = windows.borrow()[1].clone();
    new_photo::ready(&other);
    crate::files::launch::open_in(&other, vec![gio::File::for_path(&master)]);
    until(
        || other.documents.len() == 2 && !other.documents.changing.get(),
        "pinned launch target",
    );
    assert_eq!(w.documents.len(), 4);
    assert_eq!(app.windows().len(), 2);
    // Closing during preparation cancels the batch; late results never create a window.
    let signal = other
        .window
        .connect_notify_local(Some("visible-dialog"), |window, _| {
            if window
                .visible_dialog()
                .is_some_and(|d| d.widget_name() == "document-open-progress")
            {
                let weak = window.downgrade();
                glib::idle_add_local_once(move || {
                    if let Some(window) = weak.upgrade() {
                        window.close();
                    }
                });
            }
        });
    crate::files::launch::open_in(
        &other,
        vec![gio::File::for_path(&photo), gio::File::for_path(&master)],
    );
    until(
        || !other.window.is_visible(),
        "close cancels incoming batch",
    );
    pump(100);
    assert_eq!(windows.borrow().len(), 1);
    other.window.disconnect(signal);
    assert_eq!(std::fs::read(&photo).unwrap(), photo_before);
    assert_eq!(std::fs::read(&master).unwrap(), master_before);
    glib::MainContext::default()
        .block_on(w.documents.activate(&w, second))
        .unwrap();
    new_photo::ready(&w);
    crate::capture(&w, directory.join("opened-tabs.png").to_str().unwrap());
    w.window.destroy();
    windows.borrow_mut().clear();
    pump(100);
}
