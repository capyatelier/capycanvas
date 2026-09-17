//! Real GApplication argument forwarding on the private session bus. The sender
//! runs production application setup; the receiver inspects native documents.
use super::*;
use gtk::gio;
use layer_core::color::{ColorProfile, IntegerDepth, source::*};
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
            depth: IntegerDepth::U8,
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

    // Cold primary: CLI files arrive before any GPU/session exists. Multiple
    // files, including a project, create exactly their own source-sized windows.
    let mut sender = Sender::new(&id, &[&photo, &master]);
    until(
        || windows.borrow().len() == 2 && launch_window(&app).is_none(),
        "cold file list opens both documents",
    );
    sender.finish();
    let first = windows.borrow()[0].clone();
    let second = windows.borrow()[1].clone();
    new_photo::ready(&first);
    new_photo::ready(&second);
    assert_eq!(
        app.windows().len(),
        2,
        "no extra blank document or loading window"
    );
    assert!(
        activated.get(),
        "activation while loading must retain the incoming file"
    );
    {
        let gpu = first.gpu.borrow();
        let session = &gpu.as_ref().unwrap().session;
        let doc = session.engine().document();
        assert_eq!([doc.width, doc.height], expected.extent);
        assert_eq!(
            doc.color.depth,
            IntegerDepth::U16,
            "cold launch uses saved photo policy"
        );
        assert_eq!(doc.layers[0].source.as_deref(), Some(&expected));
        assert_eq!(
            doc.layers[0].properties.placement,
            layer_core::Affine::IDENTITY
        );
        assert!(doc.layers[0].raster.is_empty());
        assert!(
            session.state().document_file.location.is_none(),
            "JPEG must not become a Save destination"
        );
    }
    assert_eq!(
        second
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document(),
        &project.document
    );
    assert_eq!(
        state(&second).document_file.location.unwrap().uri,
        gio::File::for_path(&master).uri()
    );

    // A running, edited document survives another file launch unchanged, and
    // closing it still goes through the existing unsaved-work confirmation.
    new_photo::invoke(&first, CommandId::AddLayer);
    let edited = place_source::snapshot(&first);
    assert!(state(&first).document_file.modified);
    let mut sender = Sender::new(&id, &[&photo]);
    until(
        || windows.borrow().len() == 3 && launch_window(&app).is_none(),
        "warm file launch",
    );
    sender.finish();
    assert_eq!(place_source::snapshot(&first), edited);
    first.window.close();
    until(
        || first.window.visible_dialog().is_some(),
        "unsaved close prompt",
    );
    new_photo::response(&first, "cancel");
    assert!(first.window.is_visible());
    assert_eq!(place_source::snapshot(&first), edited);

    // A rejected first file is named, and acknowledgement continues to the
    // next file in the same native argument list without losing either drawing.
    let bad = directory.join("Broken picture.jpg");
    std::fs::write(&bad, b"not an image").unwrap();
    let mut sender = Sender::new(&id, &[&bad, &photo]);
    until(
        || {
            launch_window(&app)
                .and_then(|w| w.visible_dialog())
                .is_some_and(|d| d.widget_name() == "file-launch-error")
        },
        "failed file explanation",
    );
    let error = launch_window(&app)
        .unwrap()
        .visible_dialog()
        .unwrap()
        .downcast::<adw::AlertDialog>()
        .unwrap();
    assert!(error.body().contains("Broken picture.jpg"));
    click(&find_button(error.upcast_ref(), "OK").unwrap());
    until(
        || windows.borrow().len() == 4 && launch_window(&app).is_none(),
        "next file after error",
    );
    sender.finish();
    assert_eq!(place_source::snapshot(&first), edited);

    // Cancel on the first progress presentation, before accepting a worker's
    // result. Test both the dialog button and the native parent close action.
    let cancel_mode = Rc::new(Cell::new(0));
    let cancellations = Rc::new(Cell::new(0));
    let signal = app.connect_window_added(glib::clone!(
        #[strong]
        cancel_mode,
        #[strong]
        cancellations,
        move |_, window| {
            let Some(window) = window.downcast_ref::<adw::ApplicationWindow>() else {
                return;
            };
            window.connect_notify_local(
                Some("visible-dialog"),
                glib::clone!(
                    #[strong]
                    cancel_mode,
                    #[strong]
                    cancellations,
                    move |window, _| {
                        let Some(dialog) = window
                            .visible_dialog()
                            .filter(|d| d.widget_name() == "document-open-progress")
                        else {
                            return;
                        };
                        let mode = cancel_mode.replace(0);
                        if mode == 0 {
                            return;
                        }
                        let window = window.clone();
                        let cancellations = cancellations.clone();
                    glib::idle_add_local_full(glib::Priority::HIGH, move || {
                        cancellations.set(cancellations.get() + 1);
                        if mode == 1 {
                            find_button(dialog.upcast_ref(), "Cancel").unwrap().emit_clicked();
                        } else {
                            window.close();
                        }
                        glib::ControlFlow::Break
                    });
                    }
                ),
            );
        }
    ));
    for mode in [1, 2] {
        cancel_mode.set(mode);
        let count = cancellations.get();
        let mut sender = Sender::new(&id, &[&photo, &master]);
        until(
            || cancellations.get() == count + 1 && launch_window(&app).is_none(),
            "cancelled list retires after worker acknowledgement",
        );
        sender.finish();
        assert_eq!(windows.borrow().len(), 4);
        assert_eq!(place_source::snapshot(&first), edited);
    }
    app.disconnect(signal);

    // Missing-profile interpretation must also work on the launch parent, which
    // deliberately has no GPU/session of its own.
    let untagged_path = directory.join("Unprofiled photo.jpg");
    let mut untagged = photo_before[..2].to_vec();
    let mut at = 2;
    while photo_before[at + 1] != 0xda {
        let length = usize::from(u16::from_be_bytes([
            photo_before[at + 2],
            photo_before[at + 3],
        ])) + 2;
        if photo_before[at + 1] != 0xe2 {
            untagged.extend_from_slice(&photo_before[at..at + length]);
        }
        at += length;
    }
    untagged.extend_from_slice(&photo_before[at..]);
    std::fs::write(&untagged_path, &untagged).unwrap();
    for accept in [false, true] {
        let mut sender = Sender::new(&id, &[&untagged_path]);
        until(
            || {
                launch_window(&app)
                    .and_then(|w| w.visible_dialog())
                    .is_some_and(|d| d.widget_name() == "untagged-profile-dialog")
            },
            "launch profile prompt",
        );
        let dialog = launch_window(&app).unwrap().visible_dialog().unwrap();
        if accept {
            let space = find_named(dialog.upcast_ref(), "untagged-profile-space")
                .unwrap()
                .downcast::<adw::ComboRow>()
                .unwrap();
            space.set_selected(1); // Display P3
        }
        click(
            &find_button(
                dialog.upcast_ref(),
                if accept { "Use Profile" } else { "Cancel" },
            )
            .unwrap(),
        );
        until(|| launch_window(&app).is_none(), "profile completion");
        sender.finish();
        assert_eq!(windows.borrow().len(), if accept { 5 } else { 4 });
    }
    let interpreted = windows.borrow().last().unwrap().clone();
    new_photo::ready(&interpreted);
    {
        let gpu = interpreted.gpu.borrow();
        let doc = gpu.as_ref().unwrap().session.engine().document();
        assert_eq!(doc.color.space, layer_core::color::RgbSpace::DisplayP3);
        assert_eq!(doc.color.depth, IntegerDepth::U16);
        let mut expected =
            layer_color::photo::read_photo(std::io::Cursor::new(&untagged), Default::default())
                .unwrap();
        expected.interpretation.profile =
            ColorProfile::Builtin(layer_core::color::RgbSpace::DisplayP3);
        expected.interpretation.profile_assumed = false;
        assert_eq!(doc.layers[0].source.as_deref(), Some(&expected));
    }
    drop(interpreted);

    assert_eq!(std::fs::read(&photo).unwrap(), photo_before);
    assert_eq!(std::fs::read(&master).unwrap(), master_before);
    for w in windows.borrow().iter() {
        new_photo::ready(w);
    }
    crate::capture(&first, directory.join("opened-photo.png").to_str().unwrap());
    let owned = std::mem::take(&mut *windows.borrow_mut());
    for w in &owned {
        w.recovery.discard();
        w.window.destroy();
    }
    drop(owned);
    drop(first);
    drop(second);
    pump(100);
    assert!(app.windows().is_empty());
}
