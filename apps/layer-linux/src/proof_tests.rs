//! Native controls, persistence and explicit RGB delivery with a CMYK proof.
use super::new_photo::{chooser, combo, finish, invoke, ready, response};
use super::place_source::snapshot;
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace};
use std::sync::Arc;

pub(super) fn wait_proof(w: &Rc<Workspace>, prefix: &str) {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        pump(20);
        if w.proof.label.text().starts_with(prefix) {
            return;
        }
        assert!(
            !w.proof.label.text().starts_with("Proof unavailable"),
            "{:?}",
            w.proof.label.tooltip_text()
        );
        assert!(
            Instant::now() < deadline,
            "proof state: {}",
            w.proof.label.text()
        );
    }
}

#[test]
#[ignore = "isolated Wayland display and hardware GPU"]
fn native_proof_cancellation_supersession_and_failed_profile() {
    use layer_core::color::ProofRecipe;
    let app = native_test_app("art.capycanvas.ProofCancellation");
    let mut project = new_drawing(128, 64).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U8,
    };
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    let original = snapshot(&w);
    invoke(&w, CommandId::SoftProofSetup);
    super::new_photo::profile_action(&w, "proof", "builtin-0");
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), original);
    // Cancel at the worker publication boundary, even if a small LUT is fast.
    let cancelled = Rc::new(Cell::new(false));
    let observed = cancelled.clone();
    let signal = w
        .window
        .connect_notify_local(Some("visible-dialog"), move |window, _| {
            let Some(dialog) = window
                .visible_dialog()
                .filter(|d| d.widget_name() == "proof-progress")
                .and_downcast::<adw::AlertDialog>()
            else {
                return;
            };
            let observed = observed.clone();
            glib::idle_add_local_full(glib::Priority::HIGH, move || {
                observed.set(true);
                find_button(dialog.upcast_ref(), "Cancel")
                    .unwrap()
                    .emit_clicked();
                glib::ControlFlow::Break
            });
        });
    invoke(&w, CommandId::SoftProofSetup);
    super::new_photo::profile_action(&w, "proof", "builtin-0");
    click(
        &find_button(
            w.window.visible_dialog().unwrap().upcast_ref(),
            "Prepare and Apply",
        )
        .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    while !cancelled.get()
        || w.window
            .visible_dialog()
            .is_none_or(|d| d.widget_name() != "soft-proof-setup")
    {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    response(&w, "cancel");
    finish(&w);
    w.window.disconnect(signal);
    assert_eq!(snapshot(&w), original);
    let apply = |recipe| {
        let change = w
            .gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .set_proof_recipe(Some(recipe));
        w.changed(change);
    };
    apply(ProofRecipe::new(
        "First".into(),
        ColorProfile::Builtin(RgbSpace::Srgb),
    ));
    pump(5);
    apply(ProofRecipe::new(
        "Latest".into(),
        ColorProfile::Builtin(RgbSpace::AdobeRgb),
    ));
    wait_proof(&w, "Proof: Latest");
    let completed = snapshot(&w);
    let cache = w.proof.cache_info();
    invoke(&w, CommandId::GamutWarning);
    invoke(&w, CommandId::SoftProof);
    wait_proof(&w, "Gamut: Latest");
    assert_eq!(w.proof.cache_info(), cache);
    assert_eq!(snapshot(&w), completed);
    // A damaged saved recipe may exist in a portable archive. Enabling it must
    // expose failure, retire the old view and leave exact document pixels alone.
    apply(ProofRecipe::new(
        "Broken".into(),
        ColorProfile::Icc(vec![0; 132].into()),
    ));
    let failed_document = snapshot(&w);
    let deadline = Instant::now() + Duration::from_secs(10);
    while w.proof.label.text() != "Proof unavailable" {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    assert!(w.proof.label.tooltip_text().unwrap().contains("ICC"));
    assert_eq!(snapshot(&w), failed_document);
    assert!(w.proof.cache_info().is_none());
    assert!(
        !w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .rendering_suspended()
    );
    invoke(&w, CommandId::SoftProofSetup);
    let broken_setup = w
        .window
        .visible_dialog()
        .unwrap()
        .downcast::<adw::AlertDialog>()
        .unwrap();
    assert!(!broken_setup.is_response_enabled("apply"));
    assert!(broken_setup.is_response_enabled("remove"));
    response(&w, "cancel");
    finish(&w);
    assert_eq!(snapshot(&w), failed_document);
    invoke(&w, CommandId::Undo);
    wait_proof(&w, "Proof: Latest");
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .color
            .depth,
        IntegerDepth::U8
    );
    w.window.destroy();
}

/// The same benchmark fixture runs with no proof, cold preparation, then a warm
/// compare toggle. Preparation is reported separately from navigation/drawing.
pub(super) fn benchmark_proof(w: &Rc<Workspace>) -> Option<serde_json::Value> {
    let path = std::env::var_os("LAYER_BENCH_PROOF")?;
    let bytes = std::fs::read(&path).unwrap();
    let digest = glib::compute_checksum_for_data(glib::ChecksumType::Sha256, &bytes).unwrap();
    let mut recipe = layer_core::color::ProofRecipe::new(
        "Benchmark print target".into(),
        ColorProfile::Icc(bytes.into()),
    );
    let shadow_grid = std::env::var("LAYER_BENCH_PROOF_SHADOW").as_deref() == Ok("1");
    if shadow_grid {
        recipe.conversion.intent = layer_core::color::RenderingIntent::Saturation;
        recipe.conversion.black_point_compensation = false;
        recipe.simulate_black_ink = false;
    }
    let memory = || {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
            .collect::<Vec<_>>()
            .join("; ")
    };
    let before = memory();
    let start = Instant::now();
    let change = w
        .gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .set_proof_recipe(Some(recipe));
    w.changed(change);
    wait_proof(w, "Proof:");
    let cold_ms = start.elapsed().as_secs_f64() * 1000.;
    let cache = w.proof.cache_info().unwrap();
    if shadow_grid {
        assert_eq!(
            cache.1, 129,
            "shadow workload must exercise the larger cache"
        );
    }
    let after = memory();
    invoke(w, CommandId::SoftProof);
    wait_proof(w, "Normal");
    let start = Instant::now();
    w.dispatch(UiAction::Invoke {
        command: CommandId::SoftProof,
    });
    wait_proof(w, "Proof:");
    let warm_ms = start.elapsed().as_secs_f64() * 1000.;
    assert_eq!(
        w.proof.cache_info().unwrap(),
        cache,
        "warm toggle must reuse the exact LUT"
    );
    pump(100);
    Some(
        serde_json::json!({"profile": path, "sha256": digest.as_str(), "cold_ready_ms": cold_ms,
        "warm_ready_ms": warm_ms, "poll_ms": 20, "edge": cache.1, "lut_bytes": cache.2,
        "memory_before": before, "memory_ready": after,
        "simulation": if shadow_grid { "saturation, no BPC, no ink simulation" } else { "relative, BPC, black ink" }}),
    )
}
fn keyboard(w: &Rc<Workspace>, key: gdk::Key, modifiers: gdk::ModifierType) {
    let controllers = w.window.observe_controllers();
    let keys = (0..controllers.n_items())
        .find_map(|i| {
            controllers
                .item(i)
                .and_downcast::<gtk::EventControllerKey>()
                .filter(|c| c.name().as_deref() == Some("workspace-shortcuts"))
        })
        .unwrap();
    w.area.grab_focus();
    assert!(keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers]));
    keys.emit_by_name::<()>("key-released", &[&key, &0u32, &modifiers]);
    pump(100);
}
fn toggle(w: &Rc<Workspace>, name: &str) -> adw::SwitchRow {
    find_named(w.window.visible_dialog().unwrap().upcast_ref(), name)
        .unwrap()
        .downcast()
        .unwrap()
}

#[test]
#[ignore = "isolated Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_profile_picker_add_reuse_remove_and_simulation_choices() {
    use super::new_photo::{profile_action, profile_name};
    let app = native_test_app("art.capycanvas.ProfilePicker");
    let w = Workspace::with_project(&app, Some((new_drawing(128, 64).unwrap(), None)));
    w.window.present();
    ready(&w);
    let output = std::path::Path::new("../../artifacts/color-m3/profile-picker")
        .canonicalize()
        .unwrap();
    let path = output.join("Unhelpful filename.icm");
    let bytes = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::AdobeRgb)).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let expected =
        layer_color::profile_description(&ColorProfile::Icc(bytes.clone().into())).unwrap();
    invoke(&w, CommandId::SoftProofSetup);
    let setup = w
        .window
        .visible_dialog()
        .unwrap()
        .downcast::<adw::AlertDialog>()
        .unwrap();
    assert!(!setup.is_response_enabled("apply"));
    for _ in 0..2 {
        profile_action(&w, "proof", "add");
        let file = chooser();
        file.set_file(&gtk::gio::File::for_path(&path)).unwrap();
        pump(150);
        file.response(gtk::ResponseType::Accept);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !setup.is_response_enabled("apply") {
            pump(20);
            assert!(Instant::now() < deadline);
        }
        assert_eq!(profile_name(&w, "proof-profile"), expected);
    }
    // Import selects immediately and duplicate imports occupy one saved entry.
    profile_action(&w, "proof", "manage");
    let deadline = Instant::now() + Duration::from_secs(10);
    while w.window.visible_dialog().unwrap().widget_name() != "profile-library-manager" {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    let manager = w.window.visible_dialog().unwrap();
    let list = find_named(manager.upcast_ref(), "profile-library-list")
        .unwrap()
        .downcast::<gtk::ListBox>()
        .unwrap();
    assert!(list.row_at_index(0).is_some());
    assert!(list.row_at_index(1).is_none());
    response(&w, "close");
    profile_action(&w, "proof", "builtin-0");
    assert_eq!(profile_name(&w, "proof-profile"), "sRGB");
    profile_action(&w, "proof", "saved-0");
    assert_eq!(profile_name(&w, "proof-profile"), expected);
    let menu = find_named(setup.upcast_ref(), "proof-profile-choose")
        .unwrap()
        .downcast::<gtk::MenuButton>()
        .unwrap();
    menu.popup();
    pump(300);
    assert!(
        find_named(
            menu.popover().unwrap().upcast_ref(),
            "proof-profile-current"
        )
        .is_none(),
        "saved/current profile must not be duplicated"
    );
    super::new_photo::capture_ui(&w, &output, "picker.png");
    // Popovers use a separate native surface and are absent from window captures.
    let popover = menu.popover().unwrap();
    let scene = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(&popover)).snapshot(
        &scene,
        popover.width() as f64,
        popover.height() as f64,
    );
    let node = scene.to_node().expect("profile popover snapshot");
    w.window
        .renderer()
        .unwrap()
        .render_texture(&node, None)
        .save_to_png(output.join("picker-menu.png"))
        .unwrap();
    menu.popdown();
    // Removing a library copy leaves the selected bytes usable and reusable.
    profile_action(&w, "proof", "manage");
    let deadline = Instant::now() + Duration::from_secs(10);
    while w.window.visible_dialog().unwrap().widget_name() != "profile-library-manager" {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    let manager = w.window.visible_dialog().unwrap();
    let list = find_named(manager.upcast_ref(), "profile-library-list")
        .unwrap()
        .downcast::<gtk::ListBox>()
        .unwrap();
    list.select_row(list.row_at_index(0).as_ref());
    find_named(manager.upcast_ref(), "profile-library-remove")
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
    while list.row_at_index(0).is_some() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    response(&w, "close");
    assert_eq!(profile_name(&w, "proof-profile"), expected);
    profile_action(&w, "proof", "current");
    // Cancel adding a replacement leaves the current selection usable.
    profile_action(&w, "proof", "add");
    chooser().response(gtk::ResponseType::Cancel);
    while !setup.is_response_enabled("apply") {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(profile_name(&w, "proof-profile"), expected);
    for simulation in 0..3 {
        if simulation != 0 {
            invoke(&w, CommandId::SoftProofSetup);
        }
        combo(&w, "proof-simulation").set_selected(simulation);
        response(&w, "apply");
        finish(&w);
        wait_proof(&w, "Proof:");
        let proof = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .proof
            .clone()
            .unwrap();
        assert_eq!(proof.name, expected);
        assert_eq!(proof.profile, ColorProfile::Icc(bytes.clone().into()));
        assert_eq!(proof.simulate_paper, simulation == 2);
        assert_eq!(proof.simulate_black_ink, simulation != 0);
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    // A preset chosen while an added profile finishes loading wins over that
    // stale completion, including its preset label and delivery selection.
    invoke(&w, CommandId::ExportDocument);
    profile_action(&w, "export", "add");
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(&path)).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    combo(&w, "export-preset").set_selected(2);
    combo(&w, "export-preset").set_selected(0);
    let menu = find_named(
        w.window.visible_dialog().unwrap().upcast_ref(),
        "export-profile-choose",
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !menu.is_sensitive() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(profile_name(&w, "export-space"), "sRGB");
    assert_eq!(combo(&w, "export-preset").selected(), 0);
    response(&w, "cancel");
    finish(&w);
    w.window.destroy();
}

#[test]
#[ignore = "isolated Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_proof_setup_compare_history_save_reopen_and_rgb_export() {
    let app = native_test_app("art.capycanvas.PrintProof");
    let output = std::path::Path::new("../../artifacts/color-m3/gtk-journey");
    std::fs::create_dir_all(output).unwrap();
    let output = output.canonicalize().unwrap();
    let icc = std::env::var_os("LAYER_PROOF_PROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/usr/share/color/icc/krita/cmyk.icm".into());
    let mut project = new_drawing(128, 64).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    };
    project.document.layers[0].source = Some(Arc::new(super::place_source::source()));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    let original = snapshot(&w);
    let pixels = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 781))
        .unwrap()
        .bytes;
    let normal = w.gpu.borrow_mut().as_mut().unwrap().capture().unwrap();
    keyboard(
        &w,
        gdk::Key::P,
        gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SHIFT_MASK,
    );
    assert_eq!(
        w.window.visible_dialog().unwrap().widget_name(),
        "soft-proof-setup"
    );
    combo(&w, "proof-intent").set_selected(3);
    assert!(!toggle(&w, "proof-bpc").is_active());
    assert!(!toggle(&w, "proof-bpc").is_sensitive());
    combo(&w, "proof-intent").set_selected(0);
    toggle(&w, "proof-bpc").set_active(true);
    combo(&w, "proof-simulation").set_selected(2);
    for removed in [
        "proof-name",
        "proof-profile-file",
        "proof-black-ink",
        "proof-paper",
        "proof-manage-profiles",
    ] {
        assert!(find_named(w.window.visible_dialog().unwrap().upcast_ref(), removed).is_none());
    }
    super::new_photo::profile_action(&w, "proof", "add");
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(&icc)).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        pump(20);
        let row: adw::ActionRow = find_named(
            w.window.visible_dialog().unwrap().upcast_ref(),
            "proof-profile",
        )
        .unwrap()
        .downcast()
        .unwrap();
        if row.subtitle().is_some_and(|s| {
            s == layer_color::profile_description(&ColorProfile::Icc(
                std::fs::read(&icc).unwrap().into(),
            ))
            .unwrap()
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "proof profile import: {:?}",
            row.subtitle()
        );
    }
    super::new_photo::capture_ui(&w, &output, "setup.png");
    response(&w, "apply");
    finish(&w);
    wait_proof(&w, "Proof:");
    let saved_proof = snapshot(&w);
    assert_ne!(saved_proof, original);
    assert_ne!(
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .capture()
            .unwrap()
            .bytes,
        normal.bytes
    );
    let recipe = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .proof
        .clone()
        .unwrap();
    assert_eq!(
        recipe.profile,
        ColorProfile::Icc(std::fs::read(&icc).unwrap().into())
    );
    assert!(recipe.simulate_paper);
    for (key, modifiers, expected) in [
        (
            gdk::Key::p,
            gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK,
            "Normal",
        ),
        (
            gdk::Key::G,
            gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK,
            "Gamut:",
        ),
        (
            gdk::Key::p,
            gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK,
            "Proof:",
        ),
    ] {
        keyboard(&w, key, modifiers);
        wait_proof(&w, expected);
        assert_eq!(snapshot(&w), saved_proof);
        assert_eq!(
            glib::MainContext::default()
                .block_on(read_canvas_pixels(&w, 782))
                .unwrap()
                .bytes,
            pixels
        );
    }
    super::new_photo::capture_ui(&w, &output, "proof-gamut.png");
    invoke(&w, CommandId::Undo);
    ready(&w);
    assert!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .proof
            .is_none()
    );
    invoke(&w, CommandId::Redo);
    ready(&w);
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .proof
            .as_ref(),
        Some(&recipe)
    );
    invoke(&w, CommandId::SoftProof);
    wait_proof(&w, "Proof:");
    let cache = w.proof.cache_info();
    invoke(&w, CommandId::AddLayer);
    native_pen_path(&w, &[[24., 32.], [64., 30.], [100., 34.]]);
    ready(&w);
    let edited = snapshot(&w);
    invoke(&w, CommandId::Undo);
    ready(&w);
    assert_ne!(snapshot(&w), edited);
    invoke(&w, CommandId::Redo);
    ready(&w);
    assert_eq!(
        w.proof.cache_info(),
        cache,
        "drawing and history reuse the viewing transform"
    );
    // Native Save As creates an ordinary print variant, retaining exact profile bytes.
    let master = output.join(format!("Print variant-{}.capy", std::process::id()));
    invoke(&w, CommandId::SaveDocumentAs);
    let file = chooser();
    file.set_current_folder(Some(&gtk::gio::File::for_path(&output)))
        .unwrap();
    file.set_current_name(master.file_name().unwrap().to_str().unwrap());
    pump(150);
    file.response(gtk::ResponseType::Accept);
    finish(&w);
    assert!(!state(&w).document_file.modified);
    let reopened =
        layer_core::Project::read(std::fs::File::open(&master).unwrap(), Default::default())
            .unwrap();
    assert_eq!(reopened.document.proof.as_ref(), Some(&recipe));
    let restored = Workspace::with_project(
        &app,
        Some((
            reopened,
            Some(DocumentLocation {
                uri: gtk::gio::File::for_path(&master).uri().into(),
                name: "Print variant.capy".into(),
            }),
        )),
    );
    restored.window.present();
    ready(&restored);
    assert!(!state(&restored).soft_proof && !state(&restored).gamut_warning);
    invoke(&restored, CommandId::SoftProof);
    wait_proof(&restored, "Proof:");
    invoke(&restored, CommandId::GamutWarning);
    wait_proof(&restored, "Proof:");
    assert!(!state(&restored).document_file.modified);
    let delivery = output.join(format!("RGB delivery-{}.tif", std::process::id()));
    invoke(&restored, CommandId::ExportDocument);
    // The proof import must not preselect CMYK or alter delivery presets.
    assert_eq!(
        super::new_photo::profile_name(&restored, "export-space"),
        "sRGB"
    );
    combo(&restored, "export-preset").set_selected(2);
    super::new_photo::profile_action(&restored, "export", "builtin-0");
    response(&restored, "export");
    let file = chooser();
    file.set_current_folder(Some(&gtk::gio::File::for_path(&output)))
        .unwrap();
    file.set_current_name(delivery.file_name().unwrap().to_str().unwrap());
    pump(150);
    file.response(gtk::ResponseType::Accept);
    finish(&restored);
    let delivered = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(&delivery).unwrap()),
        Default::default(),
    )
    .unwrap();
    assert_eq!(delivered.interpretation.depth, IntegerDepth::U16);
    assert_eq!(
        delivered.interpretation.profile,
        ColorProfile::Icc(
            layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb))
                .unwrap()
                .into()
        )
    );
    assert!(!state(&restored).document_file.modified);
    // Compare the actual writer bytes with both viewing options disabled.
    invoke(&restored, CommandId::SoftProof);
    invoke(&restored, CommandId::GamutWarning);
    wait_proof(&restored, "Normal");
    let snapshot = {
        let gpu = restored.gpu.borrow();
        let session = &gpu.as_ref().unwrap().session;
        DocumentExport {
            project: session.capture_project_recovery().unwrap(),
            background: session.engine().view().background_rgba_linear,
            time: session.engine().animation_time(),
        }
    };
    let mut export = ExportRecipe::further_editing(DocumentColor {
        space: RgbSpace::Srgb,
        depth: IntegerDepth::U16,
    });
    export.profile = ExportProfile::builtin(RgbSpace::Srgb);
    let expected = output.join(format!("Normal delivery-{}.tif", std::process::id()));
    crate::files::export::write_snapshot(
        restored.snapshot_gpu().unwrap(),
        snapshot,
        export,
        &expected,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(delivery).unwrap(),
        std::fs::read(expected).unwrap()
    );
    restored.window.destroy();
    w.window.destroy();
}
