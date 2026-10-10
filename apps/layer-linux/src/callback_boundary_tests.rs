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
