use super::*;

fn rgb(d: &Driver, texture: &gdk::Texture, widget: &gtk::Widget, x: f32, y: f32) -> [u8; 3] {
    let b = widget.compute_bounds(&d.w.window).unwrap();
    let stride = texture.width() as usize * 4;
    let mut bytes = vec![0; stride * texture.height() as usize];
    texture.download(&mut bytes, stride);
    let i = (b.y() + y) as usize * stride + (b.x() + x) as usize * 4;
    [bytes[i + 2], bytes[i + 1], bytes[i]]
}

fn near(actual: [u8; 3], expected: HexColor) -> bool {
    actual
        .iter()
        .zip(expected.0)
        .all(|(a, e)| a.abs_diff(e) <= 2)
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_accent_preferences_input --native-storage"]
fn native_accent_preferences_input() {
    let mut d = Driver::managed("art.capycanvas.AccentPreferences");
    d.click_name("workspace-switch-painter");
    Driver::wait_ready(&d.w);
    let (_, system) = crate::workspace::system_appearance();
    let system = system.unwrap_or(DEFAULT_ACCENT);
    let red = ACCENTS[5].1;
    let custom = HexColor([0x12, 0xab, 0x56]);
    let swatch = |i: usize| format!("setting-accent-swatch-{i}");
    for theme in [Theme::Dark, Theme::Light] {
        let suffix = format!("{theme:?}").to_lowercase();
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        d.w.dispatch(UiAction::OpenSettings {
            page: SettingsPage::Appearance,
        });
        pump(400);
        d.perform(serde_json::json!([{ "point": [800.0, 990.0] }]));
        assert_eq!(state(&d.w).settings.accent, None);
        assert_eq!(state(&d.w).palette.accent, system);
        let shot = crate::snapshot(&d.w);
        shot.save_to_png(d.dir.join(format!("accent-system-{suffix}.png")))
            .unwrap();
        assert!(near(rgb(&d, &shot, &d.named(&swatch(0)), 5., 16.), system));
        assert!(near(rgb(&d, &shot, &d.named(&swatch(6)), 5., 16.), red));

        d.click_name(&swatch(6));
        assert_eq!(state(&d.w).settings.accent, Some(red));
        assert_eq!(state(&d.w).palette.accent, red);
        let pressed = d.named(&swatch(6)).downcast::<gtk::ToggleButton>().unwrap();
        assert!(pressed.is_active());
        assert!(find_named(pressed.upcast_ref(), "layer-check-symbolic").is_some());

        d.click_name(&swatch(ACCENTS.len() + 1));
        let entry = d
            .named("setting-text-accent")
            .downcast::<gtk::Entry>()
            .unwrap();
        assert!(entry.is_mapped() && entry.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN));
        assert_eq!(entry.text(), red.to_string());
        assert_eq!(
            state(&d.w).settings.accent,
            Some(red),
            "opening the editor changes nothing"
        );
        d.key(0xff57);
        for _ in 0..6 {
            d.key(0xff08);
        }
        for c in "12ab56".chars() {
            d.key(c as u32);
        }
        d.key(0xff0d);
        assert_eq!(state(&d.w).settings.accent, Some(custom));
        assert_eq!(state(&d.w).palette.accent, custom);
        assert!(entry.is_mapped());
        d.perform(serde_json::json!([{ "point": [800.0, 990.0] }]));
        let shot = crate::snapshot(&d.w);
        shot.save_to_png(d.dir.join(format!("accent-custom-{suffix}.png")))
            .unwrap();
        assert!(near(
            rgb(&d, &shot, &d.named(&swatch(ACCENTS.len() + 1)), 5., 16.),
            custom
        ));

        d.w.dispatch(UiAction::Preferences {
            action: PreferenceAction::Reveal {
                id: PreferenceId::ZenShowCapy,
            },
        });
        pump(400);
        let row = d.named("setting-zen-show-capy");
        let switch = find_type::<gtk::Switch>(&row).unwrap();
        assert!(switch.is_active());
        let shot = crate::snapshot(&d.w);
        let track = rgb(
            &d,
            &shot,
            switch.upcast_ref(),
            6.,
            switch.height() as f32 / 2.,
        );
        assert!(
            near(track, custom),
            "libadwaita widgets use the chosen accent: {track:?}"
        );

        d.w.dispatch(UiAction::Preferences {
            action: PreferenceAction::Reveal {
                id: PreferenceId::Accent,
            },
        });
        pump(300);
        d.click_name(&swatch(0));
        assert_eq!(state(&d.w).settings.accent, None);
        assert_eq!(state(&d.w).palette.accent, system);
        assert!(
            !entry
                .parent()
                .and_downcast::<gtk::Revealer>()
                .unwrap()
                .reveals_child()
        );
        d.w.dispatch(UiAction::CloseSettings);
        d.perform(serde_json::json!([{ "point": [800.0, 990.0] }]));
        pump(400);
        let palette = state(&d.w).palette;
        let shot = crate::snapshot(&d.w);
        shot.save_to_png(d.dir.join(format!("accent-workspace-{suffix}.png")))
            .unwrap();
        let pill = d.named("workspace-switch-painter");
        let sampled = rgb(&d, &shot, &pill, 8., pill.height() as f32 / 2.);
        assert!(
            near(sampled, palette.header_selection),
            "{sampled:?} {}",
            palette.header_selection
        );
    }
    d.finish();
}

fn find_type<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Option<T> {
    if let Some(found) = root.downcast_ref::<T>() {
        return Some(found.clone());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(found) = find_type(&widget) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}
