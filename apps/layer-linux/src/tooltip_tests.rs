//! Real Mutter mouse hover plus the pen tooltip controller's picked-widget path.
//! The remote-input protocol has no tablet hover injection; hardware pen is separate.
use super::*;

#[test]
#[ignore = "isolated native-input.js --tooltips"]
fn native_tooltip_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let output = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| dir.to_string_lossy().into()),
    );
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.TooltipInput");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let mut step = 0;
    let mut perform = |events: serde_json::Value| {
        std::fs::write(
            dir.join(format!("step-{step}.json")),
            serde_json::to_vec(&events).unwrap(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.join(format!("done-{step}")).exists() {
            assert!(Instant::now() < deadline);
            pump(10);
        }
        step += 1;
        pump(100);
    };
    std::fs::write(dir.join("ready"), "ready").unwrap();
    pump(400);
    fn native_tip(root: &gtk::Widget) -> Option<gtk::Widget> {
        if root.css_name() == "tooltip" && root.is_visible() {
            return Some(root.clone());
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            child = node.next_sibling();
            if let Some(t) = native_tip(&node) {
                return Some(t);
            }
        }
        None
    }
    let popup = || {
        find_named(w.window.upcast_ref(), "pen-tooltip")
            .map(|p| p.downcast::<gtk::Popover>().unwrap())
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(180);
        let id = state(&w)
            .workspace
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()[0]
            .id;
        let source = w
            .customization
            .drawer_button(TileAnchor {
                panel: Panel::Toolbar,
                tile: id,
            })
            .unwrap();
        let bounds = source.compute_bounds(&w.window).unwrap();
        let point = [
            bounds.x() + bounds.width() / 2.,
            bounds.y() + bounds.height() / 2.,
        ];
        perform(serde_json::json!([{"point":point}]));
        pump(850);
        assert!(
            native_tip(w.window.upcast_ref()).is_some(),
            "Existing mouse tooltip appears"
        );
        // The mouse stays parked at a different control while the pen hovers a tile.
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetColumnCollapsed {
                group: 8,
                collapsed: true,
            },
        });
        pump(200);
        let target = w.columns.button(8, Panel::Layers).unwrap();
        let b = target.compute_bounds(&w.window).unwrap();
        let p = [
            f64::from(b.x() + b.width() / 2.),
            f64::from(b.y() + b.height() / 2.),
        ];
        w.tooltips.hover(w.window.upcast_ref(), p);
        pump(200);
        assert!(popup().is_none(), "Pen hover waits before showing");
        pump(420);
        let tip = popup().expect("Pen tooltip appears independently of mouse position");
        assert!(tip.is_visible() && tip.is_mapped());
        assert_eq!(
            tip.child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap()
                .text(),
            "Layers"
        );
        assert!(
            native_tip(w.window.upcast_ref()).is_none(),
            "Parked mouse has no second tooltip"
        );
        let surface = tip.surface().unwrap().downcast::<gdk::Popup>().unwrap();
        let x = surface.position_x() as f32;
        let y = surface.position_y() as f32;
        assert!(
            y >= b.y() + b.height(),
            "Tooltip is below the anchor: {y} vs {}",
            b.y() + b.height()
        );
        assert!(
            x >= 0. && x + tip.width() as f32 <= w.window.width() as f32 + 1.,
            "Tooltip fits inside the right edge"
        );
        capture_popover(
            &tip,
            &output.join(format!("pen-{theme:?}.png")).to_string_lossy(),
        );
        perform(serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]));
        assert!(
            popup().is_none(),
            "Keyboard input dismisses the pen tooltip"
        );
        // Source invalidation cancels pending and visible tooltips, without activation.
        for delay in [100, 620] {
            w.tooltips.hover(w.window.upcast_ref(), p);
            pump(delay);
            target.set_visible(false);
            pump(30);
            assert!(popup().is_none());
            target.set_visible(true);
            pump(50);
        }
        // Returning to actual mouse input restores its native tooltip and policy.
        perform(serde_json::json!([{"point":[point[0]+1.,point[1]]}]));
        pump(700);
        assert!(source.has_tooltip());
        assert!(native_tip(w.window.upcast_ref()).is_some());
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetColumnCollapsed {
                group: 8,
                collapsed: false,
            },
        });
        pump(200);
        let mut child = w.layer_panel.footer.first_child();
        let action = loop {
            let button = child.expect("New layer button");
            if button.tooltip_text().as_deref() == Some("New layer") {
                break button;
            }
            child = button.next_sibling();
        };
        let b = action.compute_bounds(&w.window).unwrap();
        w.tooltips.hover(
            w.window.upcast_ref(),
            [
                f64::from(b.x() + b.width() / 2.),
                f64::from(b.y() + b.height() / 2.),
            ],
        );
        pump(620);
        let current = state(&w);
        let expected = current.settings.action_tooltip(
            "New layer",
            &UiAction::Layer {
                action: layer_ui::LayerAction::New {
                    group: false,
                    clipped: false,
                },
            },
            current.platform,
        );
        assert_eq!(
            popup()
                .unwrap()
                .child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap()
                .text(),
            expected,
            "Pen action tooltips retain the shared keyboard hint"
        );
        w.tooltips.hide();
    }
    std::fs::write(dir.join("finished"), "done").unwrap();
    w.window.destroy();
    pump(100);
}
