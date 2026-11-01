use super::*;
use layer_ui::GlassColor;
use layer_workspace::ManagerPage;

type Rgb = [f64; 3];

struct Shot {
    bytes: Vec<u8>,
    stride: usize,
}

impl Shot {
    fn new(w: &Workspace, path: &str) -> Self {
        let texture = crate::with_canvas_snapshot(w, || crate::snapshot_window(&w.window, 1.0));
        texture.save_to_png(path).unwrap();
        let stride = texture.width() as usize * 4;
        let mut bytes = vec![0; stride * texture.height() as usize];
        texture.download(&mut bytes, stride);
        Self { bytes, stride }
    }
    fn at(&self, x: f32, y: f32) -> Rgb {
        let i = y as usize * self.stride + x as usize * 4;
        [2, 1, 0].map(|c| f64::from(self.bytes[i + c]))
    }
    fn darkest(&self, x: f32, ys: std::ops::RangeInclusive<i32>) -> (f32, Rgb) {
        ys.map(|y| (y as f32, self.at(x, y as f32)))
            .min_by(|a, b| a.1.iter().sum::<f64>().total_cmp(&b.1.iter().sum()))
            .unwrap()
    }
}

fn rgb(color: HexColor) -> Rgb {
    color.0.map(f64::from)
}

fn drawn(line: GlassColor, ground: Rgb) -> Rgb {
    let [r, g, b, a] = line.0.map(f64::from);
    let ink = [r * 255., g * 255., b * 255.];
    std::array::from_fn(|i| ink[i] * a + ground[i] * (1. - a))
}

fn card(theme: Theme, ground: HexColor) -> Rgb {
    let ground = rgb(ground);
    if theme == Theme::Dark { ground.map(|v| 255. * 0.08 + v * 0.92) } else { [255.; 3] }
}

fn assert_near(what: &str, actual: Rgb, expected: Rgb) {
    assert!(
        actual.iter().zip(expected).all(|(a, e)| (a - e).abs() <= 1.5),
        "{what}: drawn {actual:?}, palette {expected:?}"
    );
}

fn bounds(widget: &gtk::Widget, w: &Workspace) -> gtk::graphene::Rect {
    widget.compute_bounds(&w.window).unwrap()
}

fn assert_first_row(w: &Workspace, shot: &Shot, list: &gtk::Widget, what: &str, theme: Theme) {
    let p = state(w).palette;
    let r = bounds(&list.first_child().unwrap(), w);
    let x = r.x() + r.width() * 0.45;
    let bottom = (r.y() + r.height()) as i32;
    let (line_y, line) = shot.darkest(x, bottom - 3..=bottom + 2);
    let ground = shot.at(x, line_y - 4.);
    assert_near(&format!("{what} card"), ground, card(theme, p.settings));
    assert_near(&format!("{what} row divider"), line, drawn(p.lines.settings.card_shade, ground));
}

#[test]
#[ignore = "GTK dialog line pixels against the shared palette: requires a Wayland display"]
fn native_dialog_lines_match_the_palette() {
    let output = std::env::var("LAYER_TEST_ARTIFACTS")
        .unwrap_or_else(|_| artifact_dir("../../artifacts/ui/dialog-lines").to_owned());
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.DialogLines");
    gtk::Settings::default().unwrap().set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    until(|| w.gpu.borrow().is_some() && w.workspaces.ready(), "dialog lines workspace ready");
    wait_workspaces(&w);
    for (theme, bases) in [
        (Theme::Dark, ["#1f1f1f", "#333333", "#3d3d3d", "#1c2c3c"]),
        (Theme::Light, ["#a4a4a4", "#b8b8b8", "#dedede", "#c0b49c"]),
    ] {
        for base in bases {
            let name = format!("{theme:?}-{}", &base[1..]).to_lowercase();
            let mut settings = state(&w).settings.clone();
            let color = HexColor::try_from(base.to_owned()).unwrap();
            if theme == Theme::Dark { settings.dark_base = color } else { settings.light_base = color }
            w.dispatch(UiAction::RestoreSettings { settings: Box::new(settings) });
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            pump(200);
            let p = state(&w).palette;
            assert_eq!(p.bg, color);

            w.workspaces.ui.show(&w, ManagerPage::Workspaces);
            let deadline = Instant::now() + Duration::from_secs(20);
            while !w.workspaces.ui.dialog.is_mapped() {
                let view = serde_json::to_value(w.workspaces.view()).unwrap();
                assert!(Instant::now() < deadline, "{name} workspaces dialog stays closed: page {} busy {} ready {} closing {} closed {} error {}",
                    view["page"], view["busy"], view["ready"], view["closing"], view["closed"], view["error"]);
                pump(20);
            }
            wait_workspaces(&w);
            pump(500);
            let manager = w.workspaces.ui.dialog.upcast_ref::<gtk::Widget>();
            let shot = Shot::new(&w, &format!("{output}/workspaces-{name}.png"));
            let rows = descendants::<gtk::ListBoxRow>(&find_named(manager, "workspace-manager-items").unwrap());
            let r = bounds(rows[1].upcast_ref(), &w);
            let x = r.x() + r.width() * 0.6;
            let top = r.y() as i32;
            let (line_y, line) = shot.darkest(x, top - 2..=top + 2);
            let ground = shot.at(x, line_y + 4.);
            assert_near(&format!("{name} workspace row divider"), line, drawn(p.lines.settings.card_shade, ground));
            let background = shot.at(r.x() + r.width() + 6., r.y() + r.height() * 0.5);
            assert_near(&format!("{name} workspaces background"), background, rgb(p.settings));
            w.workspaces.ui.close(&w);
            until(|| !w.workspaces.ui.dialog.is_mapped(), "workspaces dialog closed");
            pump(300);

            w.dispatch(UiAction::OpenSettings { page: SettingsPage::Appearance });
            pump(500);
            let dialog = w.preferences.dialog.upcast_ref::<gtk::Widget>();
            let shot = Shot::new(&w, &format!("{output}/preferences-{name}.png"));
            let mut pane = find_named(dialog, "preferences-sidebar").unwrap();
            while !pane.has_css_class("sidebar-pane") {
                pane = pane.parent().unwrap();
            }
            let b = bounds(&pane, &w);
            let (edge_x, y) = (b.x() + b.width() - 1., b.y() + b.height() * 0.85);
            let (side, content) = (shot.at(edge_x - 4., y), shot.at(edge_x + 4., y));
            assert!(
                [p.sidebar, p.sidebar_backdrop].iter().any(|&s| side.iter().zip(rgb(s)).all(|(a, e)| (a - e).abs() <= 1.)),
                "{name} sidebar {side:?}"
            );
            assert_near(&format!("{name} settings content"), content, rgb(p.settings));
            assert_near(&format!("{name} sidebar edge"), shot.at(edge_x, y), drawn(p.lines.sidebar_border, side));
            let content_view = find_named(dialog, "preferences-content").unwrap();
            assert_first_row(&w, &shot, &find_css(&content_view, "boxed-list").unwrap(), &format!("{name} preferences"), theme);
            if base == "#333333" || base == "#b8b8b8" {
                for page in SettingsPage::ALL {
                    w.dispatch(UiAction::OpenSettings { page });
                    pump(400);
                    Shot::new(&w, &format!("{output}/page-{}-{name}.png", page.key()));
                    let content_view = find_named(dialog, "preferences-content").unwrap();
                    if let Some(scroll) = descendants::<gtk::ScrolledWindow>(&content_view).into_iter().find(|scroll| scroll.is_mapped() && scroll.vadjustment().upper() > scroll.vadjustment().page_size() + 120.) {
                        scroll.vadjustment().set_value(120.);
                        pump(300);
                        Shot::new(&w, &format!("{output}/page-{}-{name}-scrolled.png", page.key()));
                        scroll.vadjustment().set_value(0.);
                    }
                }
            }
            w.dispatch(UiAction::CloseSettings);
            pump(300);

            let alert = adw::AlertDialog::builder().heading("Delete workspace?").body("Its layout cannot be restored.").build();
            alert.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
            type Opener<'a> = (&'a str, Box<dyn Fn() + 'a>);
            let opens: [Opener<'_>; 4] = [
                ("export", Box::new(|| w.dispatch(UiAction::Invoke { command: CommandId::ExportDocument }))),
                ("new-drawing", Box::new(|| w.dispatch(UiAction::Invoke { command: CommandId::NewDocument }))),
                ("color-editor", Box::new(|| crate::color_editor::show(&w, layer_ui::ColorSlot::Foreground))),
                ("alert", Box::new(|| alert.present(Some(&w.window)))),
            ];
            for (label, open) in opens {
                open();
                until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.is_mapped()), label);
                pump(500);
                let dialog = w.window.visible_dialog().unwrap();
                let shot = Shot::new(&w, &format!("{output}/{label}-{name}.png"));
                let sheet = widgets(dialog.upcast_ref()).find(|widget| widget.css_name() == "sheet" && widget.is_mapped()).unwrap();
                let s = bounds(&sheet, &w);
                assert_near(&format!("{name} {label} background"), shot.at(s.x() + 6., s.y() + s.height() * 0.5), rgb(p.settings));
                let rows = |list: &gtk::Widget| descendants::<gtk::ListBoxRow>(list).iter().filter(|row| row.is_mapped()).count();
                if let Some(list) = widgets(dialog.upcast_ref()).find(|widget| widget.has_css_class("boxed-list") && widget.is_mapped() && rows(widget) > 1) {
                    assert_first_row(&w, &shot, &list, &format!("{name} {label}"), theme);
                }
                dialog.force_close();
                until(|| w.window.visible_dialog().is_none(), label);
                pump(300);
            }

        }
    }
    w.window.close();
    pump(300);
}
