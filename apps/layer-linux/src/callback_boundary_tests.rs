use super::*;

#[test]
#[ignore = "private Wayland display and native keyboard input"]
fn native_proof_dial_callback_registration() {
    let mut driver = Driver::new("art.capycanvas.ProofCallbackRegistration");
    for theme in [Theme::Light, Theme::Dark] {
        driver.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let dial = crate::proof_dial::ProofDial::new(&driver.w.localization());
        let events = Rc::new(RefCell::new(Vec::new()));
        let registered = Cell::new(false);
        let weak = Rc::downgrade(&dial);
        let recorded = events.clone();
        dial.connect_changed(move |phase, _| {
            recorded.borrow_mut().push((phase, 1));
            if !registered.replace(true) {
                let recorded = recorded.clone();
                weak.upgrade().unwrap().connect_changed(move |phase, _| recorded.borrow_mut().push((phase, 3)));
            }
        });
        let recorded = events.clone();
        dial.connect_changed(move |phase, _| recorded.borrow_mut().push((phase, 2)));
        let window = gtk::Window::builder().transient_for(&driver.w.window).child(&dial.root).build();
        window.maximize(); window.present(); pump(250);
        assert!(dial.field.grab_focus());
        driver.input.key(0xff53);
        use layer_ui::ContactPhase::{Down, Move, Up};
        assert_eq!(*events.borrow(), [(Down, 1), (Down, 2), (Move, 1), (Move, 2), (Move, 3), (Up, 1), (Up, 2), (Up, 3)]);
        events.borrow_mut().clear();
        driver.input.key(0xff53);
        assert_eq!(*events.borrow(), [(Down, 1), (Down, 2), (Down, 3), (Move, 1), (Move, 2), (Move, 3), (Up, 1), (Up, 2), (Up, 3)]);
        crate::snapshot_window(&window, 1.).save_to_png(driver.input.dir.join(format!("proof-callback-registration-{theme:?}.png"))).unwrap();
        window.destroy(); driver.w.window.present(); pump(150);
    }
    driver.finish();
}

#[test]
#[ignore = "private Wayland display and asynchronous language publication"]
fn native_localization_callback_registration() {
    let driver = Driver::new("art.capycanvas.LocalizationCallbackRegistration");
    let switch = |language| {
        let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32;
        driver.w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: layer_ui::PreferenceValue::Choice(choice) } });
        until(|| driver.w.localization().language() == language, "callback language publication");
    };
    for theme in [Theme::Light, Theme::Dark] {
        driver.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let events = Rc::new(RefCell::new(Vec::new()));
        let armed = Rc::new(Cell::new(false));
        let active = Rc::new(Cell::new(true));
        let registered = Cell::new(false);
        let weak = Rc::downgrade(&driver.w);
        let (recorded, enabled, alive) = (events.clone(), armed.clone(), active.clone());
        driver.w.on_localization(move |_| {
            if !alive.get() { return false; }
            if enabled.get() {
                recorded.borrow_mut().push(1);
                if !registered.replace(true) {
                    let (recorded, alive) = (recorded.clone(), alive.clone());
                    weak.upgrade().unwrap().on_localization(move |_| {
                        if !alive.get() { return false; }
                        recorded.borrow_mut().push(3); true
                    });
                }
            }
            true
        });
        let (recorded, enabled, alive) = (events.clone(), armed.clone(), active.clone());
        driver.w.on_localization(move |_| {
            if !alive.get() { return false; }
            if enabled.get() { recorded.borrow_mut().push(2); }
            true
        });
        armed.set(true);
        let first = if driver.w.localization().language() == UiLanguage::French { UiLanguage::English } else { UiLanguage::French };
        switch(first);
        assert_eq!(*events.borrow(), [1, 3, 2]);
        events.borrow_mut().clear();
        switch(if first == UiLanguage::English { UiLanguage::French } else { UiLanguage::English });
        assert_eq!(*events.borrow(), [1, 2, 3]);
        driver.capture_canvas(&format!("localization-callback-registration-{theme:?}.png"));
        active.set(false);
    }
    driver.finish();
}

#[test]
#[ignore = "640px isolated compositor, --native-test=native_header_overflow_opens_while_a_photo_import_prepares"]
fn native_header_overflow_opens_while_a_photo_import_prepares() {
    let d = Driver::new("art.capycanvas.HeaderOverflowImport");
    assert!(d.w.surface.width() <= 800, "use LAYER_MOTION_VIEWPORT=640x600");
    let zone = HeaderSize::ALL.into_iter().find_map(|size| {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(240);
        (0..3).find(|i| d.named(&format!("header-overflow-{i}")).is_mapped())
    }).expect("a mapped header overflow button");
    let button = d.named(&format!("header-overflow-{zone}")).downcast::<gtk::MenuButton>().unwrap();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 640, 480);
    encoder.set_color(png::ColorType::Rgba);
    encoder.write_header().unwrap().write_image_data(&vec![180; 640 * 480 * 4]).unwrap();
    let layer_ui::ImportOutcome::Editable(imported) = layer_ui::read_import(
        std::io::Cursor::new(bytes), layer_ui::ImportIntent::Open, Default::default(),
        layer_ui::photo_document_names("Overflow.png", &d.w.localization()), Default::default(), layer_ui::photo_decode_limits(),
        &std::sync::atomic::AtomicBool::new(false),
    ).unwrap() else { panic!("editable photo") };
    let completed = Rc::new(RefCell::new(None));
    glib::spawn_future_local(glib::clone!(#[strong(rename_to = w)] d.w, #[strong] completed, async move {
        *completed.borrow_mut() = Some(w.documents.open_imported(&w, imported, None).await);
    }));
    until(|| d.w.gpu.borrow().is_none(), "the open parks the canvas while the photo prepares");
    button.popup();
    assert!(button.popover().is_some());
    button.popdown();
    until(|| completed.borrow().is_some(), "the photo opens");
    assert_eq!(completed.borrow_mut().take().unwrap(), Ok(()));
    assert!(d.w.gpu.borrow().is_some());
    d.finish();
}
