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
        let accent_row = d.named("setting-accent");
        let first = d.named(&swatch(0)).compute_bounds(&accent_row).unwrap();
        let last = d.named(&swatch(ACCENTS.len() + 1)).compute_bounds(&accent_row).unwrap();
        let (left, right) = (first.x(), accent_row.width() as f32 - last.x() - last.width());
        assert!((left - right).abs() <= 2., "accent circles are centered: {left} {right}");

        let (key, default) = match theme {
            Theme::Dark => ("dark-base", 2),
            Theme::Light => ("light-base", 1),
        };
        let base = |i: usize| format!("setting-{key}-swatch-{i}");
        let choices = theme.base_choices();
        assert!(
            d.named(&base(default))
                .downcast::<gtk::ToggleButton>()
                .unwrap()
                .is_active()
        );
        let row = d.named(&format!("setting-{key}"));
        let circle = d.named(&base(0));
        assert!(circle.is_ancestor(&row));
        assert!(
            circle.compute_bounds(&row).unwrap().height() < row.height() as f32,
            "base circles share the title row"
        );
        d.click_name(&base(0));
        assert_eq!(state(&d.w).palette.bg, choices[0]);
        d.click_name(&base(choices.len()));
        let base_entry = d
            .named(&format!("setting-text-{key}"))
            .downcast::<gtk::Entry>()
            .unwrap();
        assert!(
            base_entry
                .state_flags()
                .contains(gtk::StateFlags::FOCUS_WITHIN)
        );
        assert_eq!(base_entry.text(), choices[0].to_string());
        d.key(0xff57);
        for _ in 0..6 {
            d.key(0xff08);
        }
        for c in "445566".chars() {
            d.key(c as u32);
        }
        d.key(0xff0d);
        assert_eq!(state(&d.w).palette.bg, HexColor([0x44, 0x55, 0x66]));
        d.perform(serde_json::json!([{ "point": [800.0, 990.0] }]));
        crate::snapshot(&d.w)
            .save_to_png(d.dir.join(format!("base-custom-{suffix}.png")))
            .unwrap();
        d.click_name(&base(default));
        assert_eq!(state(&d.w).palette.bg, theme.default_base());
        d.perform(serde_json::json!([{ "point": [800.0, 990.0] }]));

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
