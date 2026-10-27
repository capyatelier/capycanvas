use super::*;
use layer_workspace::DEFAULT_WORKSPACES;

#[allow(deprecated)]
fn background(widget: &gtk::Widget) -> [u8; 4] {
    let snapshot = gtk::Snapshot::new();
    snapshot.render_background(&widget.style_context(), 0., 0., 100., 60.);
    let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 100, 60).unwrap();
    let cr = gtk::cairo::Context::new(&surface).unwrap();
    cr.set_source_rgb(0.5, 0.5, 0.5);
    cr.paint().unwrap();
    if let Some(node) = snapshot.to_node() { node.draw(&cr); }
    drop(cr);
    let stride = surface.stride() as usize;
    surface.data().unwrap()[30 * stride + 50 * 4..30 * stride + 50 * 4 + 4].try_into().unwrap()
}

#[test]
#[ignore = "private compositor and native control styles"]
fn native_button_feedback_styles() {
    let d = Driver::new("art.capycanvas.ButtonFeedbackStyles");
    let window = adw::Window::builder().transient_for(&d.w.window).default_width(500).default_height(700).build();
    window.add_css_class("capy-workspace");
    window.set_widget_name(&d.w.window.widget_name());
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("toolbar-component");
    window.set_content(Some(&body));
    let families = [
        ("choice-well", ""), ("workspace-manager-row", "workspace-pin"),
        ("workspace-manager-row", "workspace-reorder-handle"),
        ("layers-panel", "layer-thumbnail"), ("layers-panel", "layer-folder"),
        ("layers-panel", "layer-effect"), ("image-selector", ""),
        ("choice-grid", ""), ("color-panel", "color-utility"),
        ("palette-panel", "palette-tile"), ("palette-panel", "palette-add"),
        ("picker-dialog", "palette-add"), ("edit-color-pair", "edit-color-revert"),
        ("new-drawing-dialog", "new-drawing-preview"), ("new-drawing-dialog", "new-drawing-link"),
        ("new-drawing-dialog", "new-drawing-paper-tile"), ("transparency-choice", ""),
        ("layer-swipe-actions", "layer-swipe-duplicate"),
        ("picker-dialog", "palette-tile"), ("swatch-selector", ""),
        ("dock-panel", "dropdown"), ("dock-panel", "panel-choice"),
        ("new-drawing-dialog", "dropdown"), ("new-drawing-dialog", "number-value"),
        ("toolbar-component", "dropdown"),
        ("canvas-action-bar", ""), ("selection-modes", ""), ("option-action", ""),
        ("layers-panel", "layer-reference"), ("new-drawing-dialog", "new-drawing-pill"),
    ];
    let buttons: Vec<_> = families.iter().map(|(parent_class, class)| {
        let parent = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        parent.add_css_class(parent_class);
        if *parent_class == "toolbar-component" { parent.add_css_class("vertical-component"); }
        let button = if *class == "dropdown" {
            let dropdown = gtk::DropDown::from_strings(&["Choice"]);
            parent.append(&dropdown);
            dropdown.first_child().unwrap()
        } else if matches!(*class, "workspace-reorder-handle" | "panel-choice") {
            let menu = gtk::MenuButton::new();
            menu.add_css_class(class);
            parent.append(&menu);
            menu.first_child().unwrap()
        } else {
            let button = gtk::Button::new();
            if *class == "number-value" {
                let field = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                field.add_css_class("new-drawing-field");
                field.append(&button);
                parent.append(&field);
            } else { parent.append(&button); }
            button.upcast::<gtk::Widget>()
        };
        button.add_css_class("flat");
        if !class.is_empty() { button.add_css_class(class); }
        body.append(&parent);
        button
    }).collect();
    window.present();
    let mut failures = Vec::new();
    let mut rows = Vec::new();
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for flags in [gtk::StateFlags::NORMAL, gtk::StateFlags::PRELIGHT, gtk::StateFlags::ACTIVE,
            gtk::StateFlags::PRELIGHT | gtk::StateFlags::ACTIVE, gtk::StateFlags::CHECKED,
            gtk::StateFlags::CHECKED | gtk::StateFlags::PRELIGHT, gtk::StateFlags::CHECKED | gtk::StateFlags::ACTIVE,
            gtk::StateFlags::CHECKED | gtk::StateFlags::PRELIGHT | gtk::StateFlags::ACTIVE] {
            for button in &buttons { button.set_state_flags(flags, true); }
            pump(260);
            rows.push(buttons.iter().map(|button| background(button)).collect::<Vec<_>>());
        }
        for (checked, states) in rows[rows.len()-8..].chunks_exact(4).enumerate() {
            for (i, family) in families.iter().enumerate() {
                let [idle, hover, press, hover_press] = std::array::from_fn::<_, 4, _>(|s| states[s][i]);
                if idle == hover || idle == press || hover == hover_press {
                    failures.push(format!("{theme:?} {family:?} checked={checked}: idle={idle:?} hover={hover:?} press={press:?} hover_press={hover_press:?}"));
                }
            }
        }
    }
    window.close();
    d.finish();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "private compositor with native mouse, touch and optional tablet input"]
fn native_workspace_switcher_press_feedback() {
    let mut d = Driver::managed("art.capycanvas.WorkspacePressFeedback");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from);
    if let Some(output) = &output { std::fs::create_dir_all(output).unwrap(); }
    let devices: &[&str] = if std::env::var("WAYLAND_DISPLAY").is_ok_and(|display| display.ends_with("-tablet")) { &["pen"] } else { &["mouse", "touch"] };
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for device in devices {
            let id = d.w.workspaces.manager().unwrap().active_id().unwrap();
            let next = DEFAULT_WORKSPACES.iter().find(|(next, _)| *next != id).unwrap().0;
            let name = format!("workspace-switch-{}", next.rsplit(':').next().unwrap());
            let button = d.named(&name).downcast::<gtk::ToggleButton>().unwrap();
            d.input.perform(serde_json::json!([{"point":[800.,990.]}]));
            if *device == "pen" { d.input.perform(serde_json::json!([{"pen":"leave"}])); }
            pump(250);
            let idle = background(button.upcast_ref());
            let point = d.point(button.upcast_ref());
            d.input.perform(serde_json::json!([contact(device, "down", point)]));
            assert!(!button.is_active(), "{device}: workspace stays unselected until release");
            assert_eq!(d.w.workspaces.manager().unwrap().active_id().as_deref(), Some(id.as_str()));
            assert_ne!(idle, background(button.upcast_ref()), "{theme:?} {device}: visible pressed state");
            if let Some(output) = &output {
                crate::snapshot_window(&d.w.window, 1.).save_to_png(output.join(format!("{theme:?}-{device}-pressed.png"))).unwrap();
            }
            d.input.perform(serde_json::json!([contact(device, "up", point)]));
            until(|| !d.w.workspaces.busy() && d.w.workspaces.manager().unwrap().active_id().as_deref() == Some(next), "workspace activation");
            assert!(button.is_active());
            assert!(!button.state_flags().contains(gtk::StateFlags::ACTIVE));
            if let Some(output) = &output {
                crate::snapshot(&d.w).save_to_png(output.join(format!("{theme:?}-{device}-selected.png"))).unwrap();
            }

            let previous = format!("workspace-switch-{}", id.rsplit(':').next().unwrap());
            let cancelled = d.named(&previous).downcast::<gtk::ToggleButton>().unwrap();
            let start = d.point(cancelled.upcast_ref());
            let outside = [start[0], start[1]+100.];
            d.input.perform(serde_json::json!([contact(device, "down", start), contact(device, "move", outside), contact(device, "up", outside)]));
            assert!(!cancelled.is_active(), "{device}: release outside cancels selection");
            assert!(!cancelled.state_flags().contains(gtk::StateFlags::ACTIVE));
            cancelled.set_sensitive(false);
            d.input.perform(serde_json::json!([contact(device, "down", start), contact(device, "up", start)]));
            assert!(!cancelled.state_flags().contains(gtk::StateFlags::ACTIVE));
            assert!(!cancelled.is_active());
            cancelled.set_sensitive(true);
            d.input.perform(serde_json::json!([{"point":[800.,990.]}]));
            if *device == "pen" { d.input.perform(serde_json::json!([{"pen":"leave"}])); }
        }
    }
    d.finish();
}
