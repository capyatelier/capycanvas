//! New/Open/Save/Export through GTK dialogs and the production SDR worker.
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace, source::*};

pub(super) fn ready(w: &Rc<Workspace>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        pump(20);
        if w.workspaces.ready.get()
            && !w.workspaces.busy.get()
            && w.workspaces.accepts_input(w)
            && w.gpu.borrow().as_ref().is_some_and(|g| {
                g.session.engine().backend().startup.complete
                    && g.session.engine().backend().paint_ready(
                        g.session.engine().document(),
                        g.session.engine().configured_brush(),
                        false,
                    )
                    && !g.session.state().filter_load.pending
                    && !g.session.engine().has_pending_document_edits()
            })
        {
            return;
        }
        assert!(Instant::now() < deadline, "ready: {}", w.status.text());
    }
}
pub(super) fn finish(w: &Rc<Workspace>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while state(w).document_file.busy || !state(w).requests.is_empty() {
        pump(20);
        assert!(
            Instant::now() < deadline,
            "file completion: {}",
            w.status.text()
        );
    }
    assert!(state(w).host_error.is_none(), "{:?}", state(w).host_error);
}
#[allow(deprecated)]
pub(super) fn chooser() -> gtk::FileChooserDialog {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        pump(20);
        if let Some(dialog) = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::FileChooserDialog>().ok())
            .find(|w| w.is_visible())
        {
            pump(350);
            return dialog;
        }
        assert!(Instant::now() < deadline, "native chooser");
    }
}
pub(super) fn export_enabled(w: &Rc<Workspace>) -> bool {
    find_named(w.window.visible_dialog().unwrap().upcast_ref(), "export-confirm").unwrap().is_sensitive()
}
pub(super) fn export_page(w: &Rc<Workspace>, tag: &str) {
    let dialog = w.window.visible_dialog().unwrap();
    if dialog.widget_name() != "export-options" { return; }
    let nav = find_named(dialog.upcast_ref(), "export-navigation").unwrap().downcast::<adw::NavigationView>().unwrap();
    if nav.visible_page_tag().as_deref() == Some(tag) { return; }
    nav.pop_to_tag("main");
    if tag != "main" {
        let row = find_named(dialog.upcast_ref(), &format!("export-open-{tag}")).unwrap().downcast::<adw::ActionRow>().unwrap();
        row.emit_by_name::<()>("activated", &[]);
    }
    pump(350);
    assert_eq!(nav.visible_page_tag().as_deref(), Some(tag));
}
pub(super) fn controls_root(window: &adw::ApplicationWindow) -> gtk::Widget {
    window.visible_dialog().map(|d| d.upcast()).unwrap_or_else(|| window.clone().upcast())
}
pub(super) fn combo(w: &Rc<Workspace>, name: &str) -> adw::ComboRow {
    if name == "proof-intent" && w.window.visible_dialog().is_none() {
        find_named(w.proof_panel.root.upcast_ref(),"proof-advanced").unwrap().downcast::<gtk::MenuButton>().unwrap().popup();
        pump(100);
    }
    find_named(&controls_root(&w.window), name)
        .unwrap()
        .downcast()
        .unwrap()
}
pub(super) fn menu_has_action(model: &gtk::gio::MenuModel, action: &str) -> bool {
    (0..model.n_items()).any(|index| {
        model
            .item_attribute_value(index, "action", None)
            .and_then(|v| v.get::<String>())
            .as_deref()
            == Some(action)
            || ["section", "submenu"].iter().any(|link| {
                model
                    .item_link(index, link)
                    .is_some_and(|child| menu_has_action(&child, action))
            })
    })
}
pub(super) fn profile_action(w: &Rc<Workspace>, prefix: &str, action: &str) {
    profile_action_window(&w.window, prefix, action);
}

pub(super) fn profile_action_window(window: &adw::ApplicationWindow, prefix: &str, action: &str) {

    if prefix == "export" {
        let dialog = window.visible_dialog().unwrap();
        let nav = find_named(dialog.upcast_ref(), "export-navigation").unwrap().downcast::<adw::NavigationView>().unwrap();
        if nav.visible_page_tag().as_deref() != Some("color") { nav.pop_to_tag("main"); nav.push_by_tag("color"); pump(350); }
    }
    let menu = find_named(
        &controls_root(window),
        &format!("{prefix}-profile-choose"),
    )
    .unwrap()
    .downcast::<gtk::MenuButton>()
    .unwrap();
    menu.popup();
    let popup = menu
        .popover()
        .unwrap()
        .downcast::<gtk::PopoverMenu>()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let action = format!("profile.{action}");
    while !menu_has_action(&popup.menu_model().unwrap(), &action) {
        pump(10);
        assert!(
            Instant::now() < deadline,
            "profile action: {prefix}/{action}"
        );
    }
    popup.activate_action(&action, None).unwrap();
    if action != "profile.add" && action != "profile.manage" {
        while !menu.is_sensitive() {
            pump(10);
            assert!(Instant::now() < deadline, "profile read");
        }
    }
}
pub(super) fn profile_manager_action(w: &Rc<Workspace>, index: u32, action: &str) {
    let dialog = w.window.visible_dialog().unwrap();
    let menu = find_named(
        dialog.upcast_ref(),
        &format!("profile-library-menu-{index}"),
    )
    .unwrap()
    .downcast::<gtk::MenuButton>()
    .unwrap();
    menu.popup();
    pump(30);
    menu.popover()
        .unwrap()
        .activate_action(&format!("saved.{action}"), None)
        .unwrap();
    let list = find_named(dialog.upcast_ref(), "profile-library-list").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !list.is_sensitive() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
}
pub(super) fn profile_name(w: &Rc<Workspace>, name: &str) -> String {
    let root=controls_root(&w.window);
    let control=find_named(&root,name).unwrap();
    if let Some(row)=control.downcast_ref::<adw::ActionRow>() {return row.subtitle().unwrap().into();}
    find_named(&control,"proof-profile-choose").unwrap().downcast::<gtk::MenuButton>().unwrap().label().unwrap().into()
}
pub(super) fn response(w: &Rc<Workspace>, id: &str) {
    if w.window.visible_dialog().is_some_and(|d| d.widget_name() == "export-options") {
        export_page(w, "main");
        let dialog = w.window.visible_dialog().unwrap();
        let name = match id { "export" => "export-confirm", "cancel" => "export-cancel", _ => panic!("unexpected export response: {id}") };
        let widget = find_named(dialog.upcast_ref(), name).unwrap();
        assert!(widget.is_sensitive());
        assert!(widget.is_visible());
        if let Some(button) = widget.downcast_ref::<gtk::Button>() { click(button); }
        else { widget.downcast::<adw::ActionRow>().unwrap().emit_by_name::<()>("activated", &[]); }
        let deadline = Instant::now() + Duration::from_secs(5);
        while dialog.is_mapped() { pump(20); assert!(Instant::now() < deadline); }
        return;
    }
    if id == "close"
        && w.window
            .visible_dialog()
            .is_some_and(|d| d.widget_name() == "profile-library-manager")
    {
        let dialog = w.window.visible_dialog().unwrap();
        click(&find_button(dialog.upcast_ref(), "Done").unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        while dialog.is_mapped() {
            pump(20);
            assert!(Instant::now() < deadline);
        }
        return;
    }
    let dialog = w
        .window
        .visible_dialog()
        .unwrap()
        .downcast::<adw::AlertDialog>()
        .unwrap();
    assert!(dialog.is_response_enabled(id), "{id} disabled");
    click(&find_button(dialog.upcast_ref(), &dialog.response_label(id)).unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while dialog.is_mapped() {
        pump(20);
        assert!(Instant::now() < deadline, "dialog dismissal");
    }
}
pub(super) fn invoke(w: &Rc<Workspace>, command: CommandId) {
    w.dispatch(UiAction::Invoke { command });
    pump(150);
}
pub(super) fn capture_ui(w: &Rc<Workspace>, directory: &std::path::Path, name: &str) {
    capture_reference(w, directory.join(name).to_str().unwrap(), 1.);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_open_cancellation_releases_request_and_preserves_current_document() {
    let app = native_test_app("art.capycanvas.CancelOpen");
    let w = Workspace::with_project(&app, Some((new_drawing(128, 128).unwrap(), None)));
    let created = Rc::new(RefCell::new(None));
    let result = created.clone();
    *w.open_document.borrow_mut() = Some(Rc::new(move |project, location, _| {
        result.replace(Some((project, location)));
    }));
    w.window.present();
    ready(&w);
    let original = super::place_source::snapshot(&w);
    let directory = std::env::temp_dir().join(format!("capy-cancel-open-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let photo = directory.join("photo.png");
    layer_color::photo::write_png(
        std::fs::File::create(&photo).unwrap(),
        &super::place_source::source(),
    )
    .unwrap();
    let hdr = directory.join("hdr-pq.png");
    layer_color::photo::write_hdr_png_rows(std::fs::File::create(&hdr).unwrap(), [513, 257],
        layer_core::color::RgbSpace::Srgb, None, false, |_, row| {
            row.fill([4., 2., 1., 1.]); Ok(())
        }).unwrap();
    let native = directory.join("master.capy");
    std::fs::write(&native, &original).unwrap();
    let cancelled = Rc::new(Cell::new(0));
    let count = cancelled.clone();
    let signal = w
        .window
        .connect_notify_local(Some("visible-dialog"), move |window, _| {
            let Some(dialog) = window
                .visible_dialog()
                .filter(|dialog| dialog.widget_name() == "document-open-progress")
                .and_downcast::<adw::AlertDialog>()
            else {
                return;
            };
            let count = count.clone();
            // Cancel on the next owner iteration, before accepting the worker's
            // completion. This exercises the final cancellation/publication boundary
            // deterministically even when this small fixture decodes immediately.
            glib::idle_add_local_full(glib::Priority::HIGH, move || {
                count.set(count.get() + 1);
                find_button(dialog.upcast_ref(), "Cancel")
                    .unwrap()
                    .emit_clicked();
                glib::ControlFlow::Break
            });
        });
    for path in [&photo, &native, &hdr, &hdr] {
        invoke(&w, CommandId::OpenDocument);
        let file = chooser();
        file.set_file(&gtk::gio::File::for_path(path)).unwrap();
        pump(150);
        file.response(gtk::ResponseType::Accept);
        finish(&w);
        assert!(
            created.borrow().is_none(),
            "cancelled Open published a window"
        );
        assert_eq!(super::place_source::snapshot(&w), original);
        assert!(
            !w.servicing.get(),
            "reader has not acknowledged cancellation"
        );
    }
    assert_eq!(cancelled.get(), 4);
    w.window.disconnect(signal);
    // The same request path remains usable after repeated cancellation.
    invoke(&w, CommandId::OpenDocument);
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(&photo)).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    finish(&w);
    let (project, location) = created.borrow_mut().take().unwrap();
    assert!(location.is_none());
    assert_eq!(
        project.document.layers[0].source.as_deref(),
        Some(&super::place_source::source())
    );
    assert_eq!(super::place_source::snapshot(&w), original);
    w.window.destroy();
    pump(100);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_new_presets_and_profiled_photo_master() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.NewPhoto");
    let output = std::path::Path::new("../../artifacts/color-m2/new-photo-ui");
    std::fs::create_dir_all(output).unwrap();
    let output = output.canonicalize().unwrap();
    let w = Workspace::with_project(&app, Some((new_drawing(256, 256).unwrap(), None)));
    let created = Rc::new(RefCell::new(None));
    let result = created.clone();
    *w.open_document.borrow_mut() = Some(Rc::new(move |project, location, _| {
        result.replace(Some((project, location)));
    }));
    w.window.present();
    ready(&w);
    invoke(&w, CommandId::NewDocument);
    assert_eq!(combo(&w, "new-document-space").selected(), 0);
    assert_eq!(combo(&w, "new-document-depth").selected(), 0);
    combo(&w, "new-document-preset").set_selected(2);
    assert_eq!(combo(&w, "new-document-space").selected(), 1);
    assert_eq!(combo(&w, "new-document-depth").selected(), 0);
    combo(&w, "new-document-background").set_selected(1);
    for name in ["new-document-width", "new-document-height"] {
        find_named(w.window.upcast_ref(), name)
            .unwrap()
            .downcast::<adw::SpinRow>()
            .unwrap()
            .set_value(256.);
    }
    let options = NewDocumentOptions {
        extent: [256, 256],
        color: DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: SampleDepth::U8,
        },
        background: DocumentBackground::Transparent,
    };
    click(
        &find_button(
            w.window.visible_dialog().unwrap().upcast_ref(),
            "Save Preset…",
        )
        .unwrap(),
    );
    pump(100);
    find_named(
        w.window.visible_dialog().unwrap().upcast_ref(),
        "new-document-preset-name",
    )
    .unwrap()
    .downcast::<adw::EntryRow>()
    .unwrap()
    .set_text("P3 cover");
    response(&w, "save");
    assert_eq!(state(&w).settings.new_document.presets[0].options, options);
    assert_eq!(combo(&w, "new-document-preset").selected(), 5);
    find_named(w.window.upcast_ref(), "new-document-remember")
        .unwrap()
        .downcast::<gtk::CheckButton>()
        .unwrap()
        .set_active(true);
    capture_ui(&w, &output, "new-p3-preset.png");
    response(&w, "create");
    finish(&w);
    assert_eq!(state(&w).settings.new_document.defaults, options);
    assert_eq!(
        crate::preferences::load().unwrap().unwrap().new_document,
        state(&w).settings.new_document
    );
    let (painting, location) = created.borrow_mut().take().unwrap();
    assert_eq!(painting.document.color, options.color);
    assert!(!painting.document.layers[1].visible);
    assert!(location.is_none());
    invoke(&w, CommandId::NewDocument);
    assert_eq!(combo(&w, "new-document-preset").selected(), 5);
    combo(&w, "new-document-preset").set_selected(3);
    assert_eq!(combo(&w, "new-document-space").selected(), 3);
    assert_eq!(combo(&w, "new-document-depth").selected(), 1);
    capture_ui(&w, &output, "new-photo-preset.png");
    find_named(w.window.upcast_ref(), "new-document-color")
        .unwrap()
        .downcast::<adw::ExpanderRow>()
        .unwrap()
        .set_expanded(true);
    combo(&w, "new-document-space").set_selected(2);
    combo(&w, "new-document-depth").set_selected(0);
    assert_eq!(combo(&w, "new-document-preset").selected(), 0);
    pump(220);
    capture_ui(&w, &output, "new-adobe8-color-options.png");
    combo(&w, "new-document-space").set_selected(3);
    combo(&w, "new-document-depth").set_selected(1);
    response(&w, "cancel");
    finish(&w);
    assert!(created.borrow().is_none());
    assert_eq!(state(&w).settings.new_document.defaults, options);
    // A new window must allocate its renderer in the remembered native mode.
    let fresh = Workspace::new(&app);
    fresh.window.present();
    ready(&fresh);
    assert_eq!(
        fresh
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .color,
        options.color
    );
    assert_eq!(
        fresh
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .width,
        256
    );
    native_pen_path(&fresh, &[[80., 120.], [130., 120.], [180., 120.]]);
    ready(&fresh);
    assert!(
        !fresh
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers[0]
            .raster
            .wait_data()
            .unwrap()
            .tiles
            .is_empty()
    );
    fresh.window.destroy();
    pump(50);
    invoke(&w, CommandId::NewDocument);
    combo(&w, "new-document-preset").set_selected(5);
    find_named(w.window.upcast_ref(), "new-document-remove-preset")
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
    pump(100);
    assert!(state(&w).settings.new_document.presets.is_empty());
    assert_eq!(state(&w).settings.new_document.defaults, options);
    response(&w, "cancel");
    finish(&w);
    eprintln!("New presets and cancellation passed");

    let mut builder = SourceBuilder::new(
        [513, 257],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Icc(
                layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::ProPhoto))
                    .unwrap()
                    .into(),
            ),
            profile_assumed: false,
        },
        4 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..257 {
        let row: Vec<u8> = (0..513)
            .flat_map(|x| [((x * 107 + y * 31) % 65536) as u16, 32767, 50000, 55000])
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    let mut source = builder.finish().unwrap();
    source.resolution = Some(layer_core::ImageResolution {
        unit: layer_core::ResolutionUnit::Inch,
        density: [[601, 2], [300, 1]],
    });
    let source_path = output.join(format!("Developed photo-{}.tif", std::process::id()));
    layer_color::photo::write_tiff(std::fs::File::create(&source_path).unwrap(), &source).unwrap();
    let original_bytes = std::fs::read(&source_path).unwrap();
    invoke(&w, CommandId::OpenDocument);
    let open = chooser();
    open.set_file(&gtk::gio::File::for_path(&source_path))
        .unwrap();
    pump(200);
    open.response(gtk::ResponseType::Accept);
    finish(&w);
    let (project, location) = created.borrow_mut().take().unwrap();
    assert!(location.is_none());
    assert_eq!(
        project.document.color,
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16
        }
    );
    assert_eq!(project.document.layers[0].source.as_deref(), Some(&source));
    assert_eq!(project.document.resolution, source.resolution);
    let photo = Workspace::with_project(&app, Some((project, None)));
    photo.window.present();
    ready(&photo);
    assert!(state(&photo).document_file.modified);
    assert_eq!(
        state(&photo).document_file.title(),
        source_path.file_stem().unwrap().to_str().unwrap()
    );
    invoke(&photo, CommandId::DocumentProperties);
    let deadline = Instant::now() + Duration::from_secs(5);
    while photo.window.visible_dialog().is_none() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    pump(350);
    capture_ui(&photo, &output, "opened-prophoto-details.png");
    response(&photo, "done");
    finish(&photo);
    photo.dispatch(UiAction::Invoke {
        command: CommandId::Pen,
    });
    photo.dispatch(UiAction::SetBrushSize { value: 23. });
    ready(&photo);
    native_pen_path(&photo, &[[200., 120.], [230., 120.], [270., 120.]]);
    ready(&photo);
    let edited = photo
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .capture_project_recovery()
        .unwrap();
    assert!(
        !edited.document.layers[0]
            .raster
            .wait_data()
            .unwrap()
            .tiles
            .is_empty()
    );
    assert_eq!(edited.document.layers[0].source.as_deref(), Some(&source));
    assert_eq!(edited.document.resolution, source.resolution);
    let master_path = output.join(format!("Photo master-{}.capy", std::process::id()));
    invoke(&photo, CommandId::SaveDocument);
    let save = chooser();
    assert_eq!(
        save.current_name().unwrap(),
        format!(
            "{}.capy",
            source_path.file_stem().unwrap().to_str().unwrap()
        )
    );
    save.set_current_folder(Some(&gtk::gio::File::for_path(&output)))
        .unwrap();
    save.set_current_name(master_path.file_name().unwrap().to_str().unwrap());
    pump(200);
    save.response(gtk::ResponseType::Accept);
    finish(&photo);
    assert!(!state(&photo).document_file.modified);
    assert_eq!(std::fs::read(&source_path).unwrap(), original_bytes);
    let reopened = layer_core::Project::read(
        std::fs::File::open(&master_path).unwrap(),
        Default::default(),
    )
    .unwrap();
    // RasterRevision equality is publication identity. Compare the complete
    // canonical archive for byte/content equality across a fresh publication.
    let mut reopened_bytes = Vec::new();
    let mut edited_bytes = Vec::new();
    reopened.write(&mut reopened_bytes).unwrap();
    edited.write(&mut edited_bytes).unwrap();
    assert_eq!(reopened_bytes, edited_bytes);
    let restored = Workspace::with_project(
        &app,
        Some((
            reopened,
            Some(DocumentLocation {
                uri: gtk::gio::File::for_path(&master_path).uri().into(),
                name: master_path.file_name().unwrap().to_str().unwrap().into(),
            }),
        )),
    );
    restored.window.present();
    ready(&restored);
    let before = glib::MainContext::default()
        .block_on(read_canvas_pixels(&photo, 9981))
        .unwrap();
    let after = glib::MainContext::default()
        .block_on(read_canvas_pixels(&restored, 9982))
        .unwrap();
    assert_eq!(before.bytes, after.bytes);
    let delivery = output.join(format!("Photo delivery-{}.tif", std::process::id()));
    invoke(&restored, CommandId::ExportDocument);
    combo(&restored, "export-preset").set_selected(2);
    profile_action(&restored, "export", "builtin-3");
    response(&restored, "export");
    let save = chooser();
    save.set_current_folder(Some(&gtk::gio::File::for_path(&output)))
        .unwrap();
    save.set_current_name(delivery.file_name().unwrap().to_str().unwrap());
    pump(200);
    save.response(gtk::ResponseType::Accept);
    finish(&restored);
    let delivered = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(delivery).unwrap()),
        Default::default(),
    )
    .unwrap();
    assert_eq!(delivered.interpretation.depth, SampleDepth::U16);
    assert_eq!(delivered.resolution, source.resolution);
    assert_eq!(
        delivered.interpretation.profile,
        source.interpretation.profile
    );
    assert_eq!(std::fs::read(source_path).unwrap(), original_bytes);
    assert!(!state(&restored).document_file.modified);
    for window in [&restored, &photo, &w] {
        window.window.destroy();
    }
    pump(100);
}
