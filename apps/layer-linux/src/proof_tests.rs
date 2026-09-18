//! Native controls, persistence and explicit RGB delivery with a CMYK proof.
use super::new_photo::{chooser, combo, finish, invoke, ready, response};
use super::place_source::snapshot;
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace};
use std::sync::Arc;

pub(super) fn wait_proof(w: &Rc<Workspace>, prefix: &str) {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        pump(20);
        if w.proof.label.text().starts_with(prefix) {
            assert_eq!(
                w.proof.label.is_visible(),
                prefix != "Normal",
                "proof status visibility"
            );
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

fn mode(w:&Rc<Workspace>,name:&str){
    find_named(w.proof_panel.root.upcast_ref(),"proof-mode").unwrap().downcast::<adw::ToggleGroup>().unwrap().set_active_name(Some(name));
}
fn settled(w:&Rc<Workspace>){
    let deadline=Instant::now()+Duration::from_secs(40);
    loop {pump(20);let root=w.proof_panel.root.upcast_ref();
        let pending=find_named(root,"proof-preparing").is_some_and(|w|w.is_visible()) || find_named(root,"proof-profile-choose").is_some_and(|w|!w.is_sensitive());
        if !pending{break;}assert!(Instant::now()<deadline,"Proof preparation");
    }
    let error=find_named(w.proof_panel.root.upcast_ref(),"proof-setup-error").unwrap().downcast::<gtk::Label>().unwrap();
    assert!(!error.is_visible(),"{}",error.text());
}
fn proof_choice(w:&Rc<Workspace>,name:&str)->gtk::DropDown{
    find_named(w.proof_panel.root.upcast_ref(),name).unwrap().downcast().unwrap()
}

#[test]
#[ignore = "isolated Wayland display and hardware GPU"]
fn native_proof_cancellation_supersession_and_failed_profile() {
    use layer_core::color::ProofRecipe;
    let app = native_test_app("art.capycanvas.ProofCancellation");
    let mut project = new_drawing(128, 64).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U8,
    };
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    let original = snapshot(&w);
    invoke(&w, CommandId::SoftProofSetup);mode(&w,"print");
    super::new_photo::profile_action(&w, "proof", "builtin-0");
    invoke(&w, CommandId::SoftProof);settled(&w);
    assert_eq!(w.gpu.borrow().as_ref().unwrap().session.proof_panel_mode(), layer_ui::ProofMode::Off);
    assert_eq!(snapshot(&w), original);
    assert!(!w.gpu.borrow().as_ref().unwrap().session.state().soft_proof);
    wait_proof(&w, "Normal");
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
    wait_proof(&w, "Normal");
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
    invoke(&w, CommandId::SoftProofSetup);mode(&w,"print");
    pump(250);
    assert!(find_named(w.proof_panel.root.upcast_ref(),"proof-apply").is_none());
    assert!(find_named(w.proof_panel.root.upcast_ref(),"proof-remove").is_none());
    mode(&w,"off");
    finish(&w);
    assert_eq!(snapshot(&w), failed_document);
    invoke(&w, CommandId::Undo);
    mode(&w,"print");
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
        SampleDepth::U8
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
fn toggle(w: &Rc<Workspace>, name: &str) -> gtk::CheckButton {
    find_named(&super::new_photo::controls_root(&w.window), name)
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
    let output = std::path::Path::new("../../artifacts/color-m3/profile-review-2")
        .canonicalize()
        .unwrap();
    let path = output.join("Unhelpful filename.icm");
    let bytes = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::AdobeRgb)).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let expected =
        layer_color::profile_description(&ColorProfile::Icc(bytes.clone().into())).unwrap();
    invoke(&w, CommandId::SoftProofSetup);mode(&w,"print");
    let setup = w.proof_panel.root.clone();
    assert!(w.window.visible_dialog().is_none());
    assert!(find_named(setup.upcast_ref(), "proof-apply").is_none());
    assert_eq!(
        proof_choice(&w, "proof-intent").selected(),
        0
    );
    assert_eq!(
        proof_choice(&w, "proof-simulation").selected_item().and_downcast::<gtk::StringObject>().unwrap().string().as_str(),
        "Black ink"
    );
    assert!(find_named(setup.upcast_ref(), "proof-revert").is_none());
    assert!(find_named(setup.upcast_ref(), "proof-remove").is_none());
    super::new_photo::capture_ui(&w, &output, "setup.png");
    for iteration in 0..2 {
        profile_action(&w, "proof", "add");
        let file = chooser();
        if iteration != 0 {
            assert_eq!(
                file.current_folder().and_then(|f| f.path()).as_deref(),
                path.parent()
            );
        }
        file.set_file(&gtk::gio::File::for_path(&path)).unwrap();
        pump(150);
        file.response(gtk::ResponseType::Accept);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !find_named(setup.upcast_ref(), "proof-profile-choose").unwrap().is_sensitive() {
            pump(20);
            assert!(Instant::now() < deadline);
        }
        assert_eq!(profile_name(&w, "proof-profile"), expected);
    }
    // Import selects immediately and duplicate imports occupy one saved entry.
    profile_action(&w, "proof", "manage");
    let deadline = Instant::now() + Duration::from_secs(10);
    while w.window.visible_dialog().is_none_or(|d| d.widget_name() != "profile-library-manager") {
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
    pump(350);
    super::new_photo::capture_ui(&w, &output, "manager.png");
    super::new_photo::profile_manager_action(&w, 0, "show");
    response(&w, "close");
    // Hiding survives reopening the manager without changing the chosen bytes.
    let menu = find_named(setup.upcast_ref(), "proof-profile-choose")
        .unwrap()
        .downcast::<gtk::MenuButton>()
        .unwrap();
    menu.popup();
    pump(300);
    let model = menu
        .popover()
        .unwrap()
        .downcast::<gtk::PopoverMenu>()
        .unwrap()
        .menu_model()
        .unwrap();
    assert!(!super::new_photo::menu_has_action(
        &model,
        "profile.saved-0"
    ));
    assert!(!super::new_photo::menu_has_action(
        &model,
        "profile.current"
    ));
    assert_eq!(profile_name(&w, "proof-profile"), expected);
    settled(&w);
    menu.popdown();
    profile_action(&w, "proof", "manage");
    pump(250);
    super::new_photo::profile_manager_action(&w, 0, "show");
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
        !super::new_photo::menu_has_action(
            &menu
                .popover()
                .unwrap()
                .downcast::<gtk::PopoverMenu>()
                .unwrap()
                .menu_model()
                .unwrap(),
            "profile.current"
        ),
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
    while w.window.visible_dialog().is_none_or(|d| d.widget_name() != "profile-library-manager") {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    let manager = w.window.visible_dialog().unwrap();
    let list = find_named(manager.upcast_ref(), "profile-library-list")
        .unwrap()
        .downcast::<gtk::ListBox>()
        .unwrap();
    super::new_photo::profile_manager_action(&w, 0, "remove");
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
    while !find_named(setup.upcast_ref(), "proof-profile-choose").unwrap().is_sensitive() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(profile_name(&w, "proof-profile"), expected);
    for simulation in 0..3 {
        if simulation != 0 {
            invoke(&w, CommandId::SoftProofSetup);mode(&w,"print");
        }
        proof_choice(&w, "proof-simulation").set_selected(simulation);
        settled(&w);
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
fn native_embedded_proof_replacement_preserves_local_copy_and_saves_one_profile() {
    use super::new_photo::{profile_action, profile_name};
    use layer_core::color::ProofRecipe;
    let app = native_test_app("art.capycanvas.ProofPortability");
    let output = std::path::Path::new("../../artifacts/color-m3/proof-portability");
    std::fs::create_dir_all(output).unwrap();
    let output = output.canonicalize().unwrap();
    let library = std::path::PathBuf::from(std::env::var_os("LAYER_SETTINGS_FILE").unwrap())
        .parent()
        .unwrap()
        .join("color-profiles");
    assert!(!library.exists(), "run with an isolated profile library");
    let a = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::AdobeRgb)).unwrap();
    let b = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
    let a_profile = ColorProfile::Icc(a.clone().into());
    let b_profile = ColorProfile::Icc(b.clone().into());
    let a_name = layer_color::profile_description(&a_profile).unwrap();
    let b_name = layer_color::profile_description(&b_profile).unwrap();
    let a_path = library.join(format!(
        "{}.icc",
        glib::compute_checksum_for_data(glib::ChecksumType::Sha256, &a).unwrap()
    ));
    let mut project = new_drawing(128, 64).unwrap();
    project.document.proof = Some(ProofRecipe::new(a_name.clone(), a_profile.clone()));
    let original_file = output.join("embedded-original.capy");
    project
        .write(std::fs::File::create(&original_file).unwrap())
        .unwrap();
    let project = layer_core::Project::read(
        std::fs::File::open(&original_file).unwrap(),
        Default::default(),
    )
    .unwrap();
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    let original = snapshot(&w);

    // Opening and viewing the embedded recipe does not import it locally.
    invoke(&w, CommandId::SoftProofSetup);mode(&w,"print");settled(&w);
    assert_eq!(profile_name(&w, "proof-profile"), a_name);
    assert_eq!(snapshot(&w),original);assert!(!library.exists());
    // A failed local preservation must leave the previous saved recipe intact.
    std::fs::create_dir_all(library.parent().unwrap()).unwrap();
    std::fs::write(&library,b"not a directory").unwrap();
    profile_action(&w,"proof","builtin-0");
    let deadline=Instant::now()+Duration::from_secs(40);
    loop {pump(20);let issue=find_named(w.proof_panel.root.upcast_ref(),"proof-setup-error").unwrap().downcast::<gtk::Label>().unwrap();
        if issue.is_visible(){assert!(issue.text().starts_with("Could not save the previous proof profile"));break;}
        assert!(Instant::now()<deadline,"profile preservation error");
    }
    assert_eq!(snapshot(&w),original);std::fs::remove_file(&library).unwrap();
    // Adding B applies it automatically and preserves A locally first.
    let b_file=output.join("replacement.icc");std::fs::write(&b_file,&b).unwrap();
    profile_action(&w,"proof","add");let file=chooser();file.set_file(&gtk::gio::File::for_path(&b_file)).unwrap();pump(150);file.response(gtk::ResponseType::Accept);
    settled(&w);wait_proof(&w,"Proof:");
    assert_eq!(profile_name(&w,"proof-profile"),b_name);assert_eq!(std::fs::read(&a_path).unwrap(),a);
    let held_library=library.with_extension("held");

    // Save through the native dialog, then inspect the archive itself: only B
    // travels, even though both A and B remain in this machine's library.
    let master = output.join(format!("replacement-{}.capy", std::process::id()));
    invoke(&w, CommandId::SaveDocumentAs);
    let file = chooser();
    file.set_current_folder(Some(&gtk::gio::File::for_path(&output)))
        .unwrap();
    file.set_current_name(master.file_name().unwrap().to_str().unwrap());
    pump(150);
    file.response(gtk::ResponseType::Accept);
    finish(&w);
    let archive = std::fs::read(&master).unwrap();
    assert!(!archive.windows(a.len()).any(|bytes| bytes == a));
    assert_eq!(
        archive.windows(b.len()).filter(|bytes| *bytes == b).count(),
        1
    );
    let reopened = layer_core::Project::read(archive.as_slice(), Default::default()).unwrap();
    assert_eq!(reopened.document.proof.as_ref().unwrap().profile, b_profile);
    w.window.destroy();
    let restored = Workspace::with_project(&app, Some((reopened, None)));
    restored.window.present();
    ready(&restored);
    invoke(&restored, CommandId::SoftProofSetup);mode(&restored,"print");
    assert_eq!(profile_name(&restored, "proof-profile"), b_name);
    profile_action(&restored, "proof", "saved-0");
    assert_eq!(profile_name(&restored, "proof-profile"), a_name);
    profile_action(&restored, "proof", "document");
    assert_eq!(profile_name(&restored, "proof-profile"), b_name);
    mode(&restored,"off");settled(&restored);
    finish(&restored);
    restored.window.destroy();

    // Another machine receives only B; it can still use B without the library.
    std::fs::rename(&library, &held_library).unwrap();
    let reopened = layer_core::Project::read(archive.as_slice(), Default::default()).unwrap();
    let other = Workspace::with_project(&app, Some((reopened, None)));
    other.window.present();
    ready(&other);
    invoke(&other, CommandId::SoftProofSetup);mode(&other,"print");
    profile_action(&other, "proof", "document");
    assert_eq!(profile_name(&other, "proof-profile"), b_name);
    settled(&other);
    finish(&other);
    wait_proof(&other, "Proof:");
    assert!(!library.exists());
    other.window.destroy();
    std::fs::rename(&held_library, &library).unwrap();
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
        depth: SampleDepth::U16,
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
        gdk::Key::p,
        gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK,
    );
    assert!(w.window.visible_dialog().is_none());
    assert!(w.proof_panel.root.is_mapped());
    mode(&w,"print");
    proof_choice(&w, "proof-intent").set_selected(3);
    assert!(!toggle(&w, "proof-bpc").is_active());
    assert!(!toggle(&w, "proof-bpc").is_sensitive());
    proof_choice(&w, "proof-intent").set_selected(0);
    toggle(&w, "proof-bpc").set_active(true);
    proof_choice(&w, "proof-simulation").set_selected(2);
    for removed in [
        "proof-name",
        "proof-profile-file",
        "proof-black-ink",
        "proof-paper",
        "proof-manage-profiles",
    ] {
        assert!(find_named(w.proof_panel.root.upcast_ref(), removed).is_none());
    }
    super::new_photo::profile_action(&w, "proof", "add");
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(&icc)).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        pump(20);
        let name = super::new_photo::profile_name(&w, "proof-profile");
        if name == layer_color::profile_description(&ColorProfile::Icc(std::fs::read(&icc).unwrap().into())).unwrap() {
            break;
        }
        assert!(Instant::now() < deadline, "proof profile import: {name}");
    }
    super::new_photo::capture_ui(&w, &output, "setup.png");
    settled(&w);
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
            gdk::Key::Y,
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
    assert_eq!(delivered.interpretation.depth, SampleDepth::U16);
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
    assert!(!state(&restored).gamut_warning, "Proof Off disables gamut warning too");
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
        depth: SampleDepth::U16,
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
    invoke(&restored,CommandId::ExportDocument);
    super::new_photo::export_page(&restored,"color");
    let dialog=restored.window.visible_dialog().unwrap();
    find_named(dialog.upcast_ref(),"export-print-profile").unwrap().downcast::<adw::ActionRow>().unwrap().emit_by_name::<()>("activated",&[]);
    let deadline=Instant::now()+Duration::from_secs(30);
    while !find_named(dialog.upcast_ref(),"export-print-profile").unwrap().is_sensitive(){pump(20);assert!(Instant::now()<deadline);}
    assert_eq!(super::new_photo::profile_name(&restored,"export-space"),restored.gpu.borrow().as_ref().unwrap().session.engine().document().proof.as_ref().unwrap().name);
    assert_eq!(combo(&restored,"export-format").selected(),1);
    assert!(find_named(dialog.upcast_ref(),"export-bpc").unwrap().downcast::<adw::SwitchRow>().unwrap().is_active());
    super::new_photo::capture_ui(&restored,&output,"print-profile-delivery.png");response(&restored,"cancel");finish(&restored);

    restored.window.destroy();
    w.window.destroy();
}

#[test]
#[ignore = "isolated Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_open_and_profile_pickers_remember_separate_folders() {
    use super::new_photo::profile_action;
    let app = native_test_app("art.capycanvas.FileFolders");
    let w = Workspace::with_project(&app, Some((new_drawing(64, 64).unwrap(), None)));
    *w.open_document.borrow_mut() = Some(Rc::new(|_, _, _| {}));
    w.window.present();
    ready(&w);
    let root = std::env::temp_dir().join(format!("capy-picker-folders-{}", std::process::id()));
    let artwork = root.join("artwork");
    let profiles = root.join("profiles");
    std::fs::create_dir_all(&artwork).unwrap();
    std::fs::create_dir_all(&profiles).unwrap();
    let drawing = artwork.join("drawing.capy");
    std::fs::write(&drawing, snapshot(&w)).unwrap();
    let profile = profiles.join("printer.icm");
    std::fs::write(
        &profile,
        layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb)).unwrap(),
    )
    .unwrap();
    invoke(&w, CommandId::OpenDocument);
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(&drawing)).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    finish(&w);
    invoke(&w, CommandId::SoftProofSetup);mode(&w,"print");
    profile_action(&w, "proof", "add");
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(&profile)).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    let setup = w.proof_panel.root.clone();
    assert!(w.window.visible_dialog().is_none());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !find_named(setup.upcast_ref(), "proof-profile-choose").unwrap().is_sensitive() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    mode(&w,"off");settled(&w);
    finish(&w);
    invoke(&w, CommandId::OpenDocument);
    let file = chooser();
    assert_eq!(
        file.current_folder().and_then(|f| f.path()),
        Some(artwork.clone())
    );
    file.set_current_folder(Some(&gtk::gio::File::for_path(&profiles)))
        .unwrap();
    file.response(gtk::ResponseType::Cancel);
    finish(&w);
    // A second window reloads the saved locations; cancellation does not replace them.
    let other = Workspace::with_project(&app, Some((new_drawing(64, 64).unwrap(), None)));
    other.window.present();
    ready(&other);
    invoke(&other, CommandId::OpenDocument);
    let file = chooser();
    assert_eq!(file.current_folder().and_then(|f| f.path()), Some(artwork));
    file.response(gtk::ResponseType::Cancel);
    finish(&other);
    invoke(&other, CommandId::SoftProofSetup);mode(&other,"print");
    profile_action(&other, "proof", "manage");
    pump(250);
    let manager = other.window.visible_dialog().unwrap();
    find_named(manager.upcast_ref(), "profile-library-import")
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
    let file = chooser();
    assert_eq!(
        file.current_folder().and_then(|f| f.path()),
        Some(profiles.clone())
    );
    file.response(gtk::ResponseType::Cancel);
    pump(200);
    response(&other, "close");
    profile_action(&other, "proof", "add");
    let file = chooser();
    assert_eq!(file.current_folder().and_then(|f| f.path()), Some(profiles));
    file.response(gtk::ResponseType::Cancel);
    pump(200);
    mode(&other,"off");settled(&other);
    finish(&other);
    other.window.close();
    w.window.close();
    pump(100);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "real desktop portal; set CAPY_TEST_DESKTOP_PORTAL=1, without no-portals"]
#[allow(deprecated)]
fn native_desktop_file_picker_uses_portal() {
    assert!(std::env::var_os("CAPY_TEST_DESKTOP_PORTAL").is_some());
    assert!(
        !std::env::var("GDK_DEBUG")
            .unwrap_or_default()
            .contains("no-portals")
    );
    let app = native_test_app("art.capycanvas.DesktopPickerCheck");
    let window = adw::ApplicationWindow::builder()
        .application(&*app)
        .title("File picker check")
        .default_width(360)
        .default_height(120)
        .build();
    window.set_content(Some(&gtk::Label::new(Some(
        "Checking desktop file dialogs…",
    ))));
    window.present();
    pump(300);
    eprintln!(
        "Desktop decoration layout: {:?}",
        gtk::Settings::default().unwrap().gtk_decoration_layout()
    );
    for (title, suffix) in [("Open Drawing", "capy"), ("Add Profile", "icc")] {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(title));
        filter.add_suffix(suffix);
        let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(title)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        dialog.set_initial_folder(Some(&gtk::gio::File::for_path(
            std::env::current_dir().unwrap(),
        )));
        let cancel = gtk::gio::Cancellable::new();
        let result = Rc::new(RefCell::new(None));
        let completed = result.clone();
        dialog.open(Some(&window), Some(&cancel), move |r| {
            completed.replace(Some(r));
        });
        pump(1500);
        assert!(
            result.borrow().is_none(),
            "picker failed before cancellation: {:?}",
            result.borrow()
        );
        assert!(
            !gtk::Window::list_toplevels()
                .iter()
                .any(|w| w.is_visible() && w.is::<gtk::FileChooserDialog>()),
            "desktop picker fell back to an in-process file dialog"
        );
        cancel.cancel();
        let deadline = Instant::now() + Duration::from_secs(5);
        while result.borrow().is_none() {
            pump(20);
            assert!(Instant::now() < deadline);
        }
        assert!(result.borrow().as_ref().unwrap().is_err());
    }
    window.close();
    pump(100);
}

#[test]
#[ignore = "isolated Wayland display and GPU"]
fn native_proof_panel_layout_preview_and_immediate_tab_drag() {
    let output=std::path::Path::new("../../artifacts/color-m4/local-tone/layout");
    std::fs::create_dir_all(output).unwrap();
    let app=native_test_app("art.capycanvas.ProofPanel");
    let mut p=new_drawing(512,384).unwrap();p.document.color.depth=SampleDepth::F16;
    let w=Workspace::with_project(&app,Some((p,None)));w.window.maximize();w.window.present();ready(&w);
    for preset in [layer_ui::WorkspacePreset::Illustrator,layer_ui::WorkspacePreset::Photographer] {
        let mut workspace=state(&w).workspace;
        workspace.layout=preset.layout(layer_ui::Platform::Gtk);
        w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(workspace)});pump(200);
        invoke(&w,CommandId::SdrRendition);pump(300);
        assert_eq!(state(&w).workspace.layout.panel_group(Panel::Proof),state(&w).workspace.layout.panel_group(Panel::Color));
        assert!(w.window.visible_dialog().is_none());
        let viewport=w.proof_panel.root.parent().unwrap();
        assert!(w.proof_panel.root.is_mapped());
        assert!(w.proof_panel.root.width()<=viewport.width(),"{} Proof width {} > {}",preset.name(),w.proof_panel.root.width(),viewport.width());
        for name in ["sdr-appearance-exposure","sdr-tone-pad-surface","sdr-tone-pad-tone","sdr-tone-pad-detail","sdr-appearance-highlight_color","proof-mode"] {
            let widget=find_named(w.proof_panel.root.upcast_ref(),name).unwrap();
            let b=widget.compute_bounds(&viewport).unwrap();
            assert!(b.x()>=0. && b.x()+b.width()<=viewport.width() as f32+1.,"{name}: {b:?} vs {}",viewport.width());
            assert!(b.y()>=0. && b.y()+b.height()<=viewport.height() as f32+1.,"{name} should fit without scrolling: {b:?} vs {}",viewport.height());
        }
        let before=snapshot(&w);
        let control=find_named(w.proof_panel.root.upcast_ref(),"sdr-appearance-exposure").unwrap().downcast::<crate::number_control::NumberControl>().unwrap();
        control.set_value(-1.);control.emit_by_name::<()>("value-changed",&[]);pump(50);
        let edited=snapshot(&w);assert_ne!(edited,before);
        let mode=find_named(w.proof_panel.root.upcast_ref(),"proof-mode").unwrap().downcast::<adw::ToggleGroup>().unwrap();
        mode.set_active_name(Some("off"));pump(50);assert!(!state(&w).preview_sdr);assert_eq!(snapshot(&w),edited);
        mode.set_active_name(Some("sdr"));pump(300);assert!(state(&w).preview_sdr);
        super::new_photo::capture_ui(&w,output,&format!("{}-sdr.png",preset.name()));
        mode.set_active_name(Some("print"));pump(300);
        assert!(w.proof_panel.root.width()<=viewport.width(),"Print controls must fit {}",preset.name());
        super::new_photo::capture_ui(&w,output,&format!("{}-print-compact.png",preset.name()));
        assert!(find_named(w.proof_panel.root.upcast_ref(),"proof-options").is_none());
        for name in ["proof-profile-choose", "proof-simulation", "proof-intent", "proof-bpc", "proof-gamut-warning"] {
            let control=find_named(w.proof_panel.root.upcast_ref(),name).unwrap();
            assert!(control.is_mapped(), "{name} must be directly visible");
            let bounds=control.compute_bounds(&viewport).unwrap();
            assert!(bounds.x()>=0. && bounds.x()+bounds.width()<=viewport.width() as f32+1., "{name} must fit the compact panel: {bounds:?}");
        }
        super::new_photo::capture_ui(&w,output,&format!("{}-print.png",preset.name()));
        invoke(&w,CommandId::SdrRendition);
        let tab=w.groups.borrow().iter().flat_map(|g| &g.tabs).find(|(p,_)| *p==Panel::Proof).unwrap().1.clone();
        let drag=begin_workspace_drag(&w,tab.upcast_ref(),10.,10.);
        let bounds=tab.compute_bounds(&w.surface).unwrap();
        drag.update([f64::from(w.surface.width())*0.5-f64::from(bounds.x()+10.),150.]);
        assert!(state(&w).workspace.layout.floating.iter().any(|f| matches!(&f.root, DockNode::Tabs {panels,..} if panels.contains(&Panel::Proof))),"Proof tab drags without a hold");
        drag.end();pump(250);
        assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().document().sdr_rendition.exposure,-1.);
        assert_eq!(snapshot(&w),edited);
        super::new_photo::capture_ui(&w,output,&format!("{}-floating.png",preset.name()));
        invoke(&w,CommandId::Undo);pump(100);
        // Undo restores authored data but advances the document revision.
        let normalize=|bytes:&[u8]| { let mut project=layer_core::Project::read(bytes, Default::default()).unwrap(); project.document.revision=0; let mut bytes=Vec::new(); project.write(&mut bytes).unwrap(); bytes };
        assert_eq!(normalize(&snapshot(&w)),normalize(&before));
    }
    w.window.destroy();pump(100);
}

#[test]
#[ignore = "isolated Wayland display and GPU"]
fn native_proof_toggle_remembers_mode_and_reveals_hidden_panel() {
    use layer_ui::ProofMode;
    let app = native_test_app("art.capycanvas.ProofToggle");
    let mut project = new_drawing(256, 192).unwrap();
    project.document.color.depth = SampleDepth::F16;
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let hide = || {
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetPanelVisible { panel: Panel::Proof, visible: false },
        });
        pump(100);
        assert!(state(&w).workspace.layout.panel_group(Panel::Proof).is_none());
        assert!(!w.proof_panel.root.is_mapped());
    };
    let check = |expected| {
        let gpu = w.gpu.borrow();
        let s = &gpu.as_ref().unwrap().session;
        assert_eq!(s.proof_mode(), expected);
        assert_eq!(s.command(CommandId::SoftProof).selected, expected != ProofMode::Off);
        assert_eq!(s.proof_panel_mode(), expected);
    };
    let shortcut = || keyboard(&w, gdk::Key::p, gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK);
    hide();
    let original = snapshot(&w);
    invoke(&w, CommandId::SoftProof);
    pump(150);
    check(ProofMode::Sdr);
    assert!(w.proof_panel.root.is_mapped());
    assert_eq!(snapshot(&w), original);
    mode(&w, "print");
    super::new_photo::profile_action(&w, "proof", "builtin-0");
    settled(&w);
    check(ProofMode::Print);
    let saved = snapshot(&w);
    for (name, expected) in [("print", ProofMode::Print), ("sdr", ProofMode::Sdr)] {
        mode(&w, name);
        pump(50);
        hide();
        shortcut();
        check(ProofMode::Off);
        assert!(state(&w).workspace.layout.panel_group(Panel::Proof).is_none(), "disabling leaves the panel hidden");
        shortcut();
        pump(150);
        check(expected);
        assert!(w.proof_panel.root.is_mapped(), "enabling reveals the active Proof tab");
        let selector = find_named(w.proof_panel.root.upcast_ref(), "proof-mode").unwrap().downcast::<adw::ToggleGroup>().unwrap();
        assert_eq!(selector.active_name().as_deref(), Some(name));
        mode(&w, "off");
        check(ProofMode::Off);
        invoke(&w, CommandId::SoftProof);
        check(expected);
        assert_eq!(snapshot(&w), saved, "view changes never enter document history");
    }
    // A collapsed tab group must open its native drawer when enabling.
    let group = state(&w).workspace.layout.panel_group(Panel::Proof).unwrap();
    invoke(&w, CommandId::SoftProof);
    w.dispatch(UiAction::Customize { action: CustomizationAction::SetColumnCollapsed { group, collapsed: true } });
    let column = state(&w).workspace.layout.collapsed_column_for_group(group).unwrap();
    w.dispatch(UiAction::Customize { action: CustomizationAction::SetColumnDrawers { column, drawers: true } });
    shortcut();
    pump(150);
    check(ProofMode::Sdr);
    assert!(state(&w).customization.column_drawers.iter().any(|d| matches!(d.anchor, layer_ui::DrawerAnchor::Column { group: g, origin: Panel::Proof, .. } if g == group)));
    assert_eq!(snapshot(&w), saved);
    w.window.destroy();
    pump(100);
}
