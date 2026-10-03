//! Tagged effect definitions through retained GTK controls and native archives.
use super::new_photo::{capture_ui, combo, ready, response};
use super::place_source::snapshot;
use super::*;
use layer_core::{
    EffectValue, GradientStop,
    color::{DocumentColor, SampleDepth, RgbColor, RgbSpace},
};
use layer_ui::{ColorAction, ColorInputModel, EffectAction};

fn press(w: &Rc<Workspace>, name: &str) {
    find_named(w.window.upcast_ref(), name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
    pump(100);
}
fn field(w: &Rc<Workspace>, index: usize, text: &str) {
    named::<adw::EntryRow>(w.window.visible_dialog().unwrap().upcast_ref(), &format!("edit-color-value-{index}"))
    .set_text(text);
    pump(20);
}
fn value(w: &Rc<Workspace>, key: &str) -> EffectValue {
    state(w)
        .layer_properties
        .controls
        .into_iter()
        .find(|c| c.key == key)
        .unwrap()
        .value
}
fn curve_points(w: &Rc<Workspace>, key: &str) -> Vec<[f32; 2]> {
    let session = ui_session(w);
    let document = session.engine().document();
    let EffectValue::Curve(points) = document.layer(document.active_layer).unwrap().effect.as_ref().unwrap().value(key).unwrap() else { panic!("curve property") };
    points.clone()
}
fn curve_graph(w: &Rc<Workspace>, key: &str) -> gtk::Widget {
    let graph = find_named(w.window.upcast_ref(), &format!("property-{key}-graph")).unwrap();
    graph.grab_focus(); pump(100);
    graph
}
fn curve_spin(w: &Rc<Workspace>, key: &str, axis: &str) -> gtk::SpinButton {
    let field = find_named(w.window.upcast_ref(), &format!("property-{key}-{axis}")).unwrap();
    descendant::<gtk::SpinButton>(&field).unwrap()
}
fn assert_curve_readout_visible(spin: &gtk::SpinButton) {
    let text = descendant::<gtk::Text>(spin).unwrap();
    let width = text.create_pango_layout(Some(&spin.text())).pixel_size().0;
    assert!(text.width() >= width, "complete curve readout {}: text {}px in {}px", spin.text(), width, text.width());
}
fn choose_curve_option(w: &Rc<Workspace>, native: &mut RemoteInput, drop: &gtk::DropDown, index: u32) {
    let mut parent = drop.parent();
    while let Some(widget) = parent {
        if let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>() {
            let bounds = drop.compute_bounds(scroll).unwrap();
            let adjustment = scroll.vadjustment();
            adjustment.set_value(adjustment.value() + (bounds.y() as f64).min(0.)
                + ((bounds.y() + bounds.height()) as f64 - scroll.height() as f64).max(0.));
        }
        parent = widget.parent();
    }
    pump(100);
    native.click(screen_point(drop.upcast_ref(), &w.window, [0.5, 0.5]));
    let text = drop.model().unwrap().item(index).unwrap().downcast::<gtk::StringObject>().unwrap().string();
    let option = widgets(drop.upcast_ref()).find(|widget| widget.is_mapped()
        && widget.native().is_some_and(|native| native.is::<gtk::Popover>())
        && widget.downcast_ref::<gtk::Label>().is_some_and(|label| label.text() == text))
        .unwrap_or_else(|| panic!("native choice {text} is visible"));
    native.click(screen_point(&option, &w.window, [0.5, 0.5]));
    assert_eq!(drop.selected(), index);
}
fn set(w: &Rc<Workspace>, key: &str, value: EffectValue) {
    w.dispatch(UiAction::Effect {
        action: EffectAction::Set {
            layer: state(w).layer_properties.layer.unwrap(),
            key: key.into(),
            value,
        },
    });
    ready(w);
}

fn assert_gradient_pixels(w: &Rc<Workspace>, bar: &gtk::Widget, stops: &[GradientStop]) {
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(bar)).snapshot(
        &snapshot,
        bar.width().into(),
        bar.height().into(),
    );
    let texture = w
        .window
        .renderer()
        .unwrap()
        .render_texture(&snapshot.to_node().unwrap(), None);
    let mut download = gdk::TextureDownloader::new(&texture);
    download.set_color_state(&w.view_color().state());
    download.set_format(gdk::MemoryFormat::R32g32b32a32Float);
    let (bytes, stride) = download.download_bytes();
    let space = state(w).colors.rgb_space();
    let a = stops[0].color.encoded_in(space).unwrap();
    let b = stops[1].color.encoded_in(space).unwrap();
    for fraction in [0.1, 0.3, 0.6, 0.9] {
        let x = (6. + (bar.width() - 13) as f32 * fraction).round() as usize;
        let t = (x - 6) as f64 / (bar.width() - 13) as f64;
        let rgba: [f64; 4] =
            std::array::from_fn(|c| f64::from(a[c]) * (1. - t) + f64::from(b[c]) * t);
        let encoded = space.convert(w.view_color().space(), rgba[..3].try_into().unwrap());
        let cell = layer_ui::TRANSPARENCY_CHECKER_CELL as usize;
        let checker = f64::from(crate::display_color::checker_linear()[(x - 6) / cell % 2]);
        let expected: [f64; 3] = encoded.map(|c| {
            let view = w.view_color().space();
            view.encode(view.decode(c) * rgba[3] + checker * (1. - rgba[3]))
                .clamp(0., 1.)
        });
        let pixel = &bytes[4 * stride + x * 16..][..16];
        for c in 0..4 {
            let actual = f32::from_ne_bytes(pixel[c * 4..c * 4 + 4].try_into().unwrap()) as f64;
            let expected = if c == 3 { 1. } else { expected[c] };
            assert!(
                (actual - expected).abs() < 0.004,
                "managed ramp x={x} c={c}: {actual} vs {expected}"
            );
        }
    }
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_effect_colors_gradients_and_retained_controls() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.EffectColors");
    let mut project = new_drawing(128, 128, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    let original =
        RgbColor::new(RgbSpace::DisplayP3, [0.95, 0.12, 0.234567, 123. / 65535.]).unwrap();
    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: "black_white".into(),
        },
    });
    let bucket = find_named(w.window.upcast_ref(), "tint-color-bucket").expect("each color parameter has its bucket");
    let label = bucket.parent().and_then(|line| line.parent()).and_then(|row| row.first_child());
    assert_eq!(label.and_downcast::<gtk::Label>().map(|l| l.text()).as_deref(), Some("Tint color"));
    w.dispatch(UiAction::SetColor { rgba: [0.2, 0.5, 0.1, 1.] });
    bucket.downcast::<gtk::Button>().unwrap().emit_clicked();
    ready(&w);
    assert_eq!(value(&w, "tint_color"), EffectValue::Color(state(&w).colors.definition()));
    set(&w, "tint_color", EffectValue::Color(original));
    let before = snapshot(&w);
    press(&w, "effect-color-tint_color");
    for i in 0..ColorInputModel::ALL.len() {
        combo(&w, "edit-color-model").set_selected(i as u32);
    }
    response(&w, "apply");
    ready(&w);
    assert_eq!(value(&w, "tint_color"), EffectValue::Color(original));
    assert!(snapshot(&w) == before, "untouched models create no edit");
    press(&w, "effect-color-tint_color");
    field(&w, 0, "NaN");
    assert!(
        !w.window
            .visible_dialog()
            .unwrap()
            .downcast::<adw::AlertDialog>()
            .unwrap()
            .is_response_enabled("apply")
    );
    response(&w, "cancel");
    assert_eq!(snapshot(&w), before);
    press(&w, "effect-color-tint_color");
    field(&w, 0, "0.1234567");
    response(&w, "apply");
    ready(&w);
    let edited = value(&w, "tint_color");
    assert!(
        matches!(&edited, EffectValue::Color(c) if c.space == RgbSpace::ProPhoto && c.rgba[0] == 0.1234567 && c.rgba[3] == original.rgba[3])
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    ready(&w);
    let current =
        layer_core::Project::read(std::io::Cursor::new(snapshot(&w)), Default::default()).unwrap();
    let mut previous =
        layer_core::Project::read(std::io::Cursor::new(&before), Default::default()).unwrap();
    previous.document.revision = current.document.revision;
    assert_eq!(
        current, previous,
        "undo restores all stored state; revision remains monotonic"
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    ready(&w);
    assert_eq!(value(&w, "tint_color"), edited);

    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: "gradient_map".into(),
        },
    });
    let stops = vec![
        GradientStop {
            position: 0.,
            color: original,
        },
        GradientStop {
            position: 1.,
            color: RgbColor::new(RgbSpace::AdobeRgb, [0.15, 0.6, 0.9, 0.8]).unwrap(),
        },
    ];
    set(&w, "gradient", EffectValue::Gradient(stops.clone()));
    press(&w, "effect-gradient-color");
    field(&w, 3, "37");
    response(&w, "apply");
    ready(&w);
    let EffectValue::Gradient(mut accepted) = value(&w, "gradient") else {
        panic!()
    };
    assert_eq!(
        accepted[0].color,
        RgbColor { linear_rgb: None,
            rgba: [original.rgba[0], original.rgba[1], original.rgba[2], 0.37],
            ..original
        }
    );
    assert_eq!(accepted[1], stops[1]);
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    ready(&w);
    assert_eq!(value(&w, "gradient"), EffectValue::Gradient(stops));
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    ready(&w);
    let bar = find_named(w.window.upcast_ref(), "effect-gradient").unwrap();
    assert_gradient_pixels(&w, &bar, &accepted);
    let controllers = bar.observe_controllers();
    let gesture = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
        .unwrap();
    gesture.emit_by_name::<()>("pressed", &[&1i32, &(bar.width() as f64 * 0.5), &10f64]);
    ready(&w);
    accepted.insert(
        1,
        GradientStop {
            position: 0.5,
            color: layer_core::gradient_value(&accepted, 0.5, RgbSpace::ProPhoto).unwrap(),
        },
    );
    assert_eq!(
        value(&w, "gradient"),
        EffectValue::Gradient(accepted.clone())
    );
    let saved = snapshot(&w);
    let reopened = Workspace::with_project(
        &app,
        Some((
            layer_core::Project::read(std::io::Cursor::new(saved.clone()), Default::default())
                .unwrap(),
            None,
        )),
    );
    reopened.window.present();
    ready(&reopened);
    assert_eq!(snapshot(&reopened), saved);
    assert_eq!(
        value(&reopened, "gradient"),
        EffectValue::Gradient(accepted)
    );

    // The retained brush control uses the same tagged draft and keeps opacity.
    w.dispatch(UiAction::Color {
        action: ColorAction::Definition { color: original },
    });
    w.dispatch(UiAction::SetBrushOpacity { value: 0.23 });
    let controls = gtk::Box::new(gtk::Orientation::Vertical, 8);
    controls.append(&w.color.widget);
    controls.append(&w.customization.color_pair(&w, 32));
    let window = gtk::Window::builder()
        .application(&*app)
        .child(&controls)
        .build();
    window.present();
    pump(100);
    w.color.widget.emit_clicked();
    pump(100);
    for i in 0..ColorInputModel::ALL.len() {
        combo(&w, "edit-color-model").set_selected(i as u32);
    }
    response(&w, "apply");
    assert_eq!(state(&w).colors.definition(), original);
    assert_eq!(state(&w).brush.opacity, 0.23);
    assert_eq!(snapshot(&w), saved);
    window.destroy();
    controls.remove(&w.color.widget);

    let output = std::path::PathBuf::from(format!(
        "../../artifacts/color-m2/effect-color-ui/{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&output).unwrap();
    reopened.customize(CustomizationAction::SetPanelVisible {
        panel: Panel::Properties,
        visible: true,
    });
    reopened.dispatch(UiAction::SelectPanelTab {
        group: state(&reopened)
            .workspace
            .layout
            .panel_group(Panel::Properties)
            .unwrap(),
        panel: Panel::Properties,
    });
    pump(150);
    capture_ui(&reopened, &output, "managed-gradient-reopened.png");
    press(&reopened, "effect-gradient-color");
    capture_ui(&reopened, &output, "managed-gradient-numeric.png");
    response(&reopened, "cancel");
    assert_eq!(snapshot(&reopened), saved);
    reopened.window.destroy();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "private Wayland display, hardware GPU and native pointer/keyboard"]
fn native_curve_graph_numbers_pages_and_history() {
    use serde_json::json;
    let app = native_test_app("art.capycanvas.CurveControls");
    let w = Workspace::with_project(&app, Some((new_drawing_at(128, 128, SampleDepth::F32), None)));
    w.window.set_default_size(1000, 760);
    w.window.maximize();
    w.window.present();
    ready(&w);
    if let Ok(theme) = std::env::var("CAPY_NATIVE_TEST_THEME") {
        let theme = if theme == "dark" { Theme::Dark } else { Theme::Light };
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        assert_eq!(state(&w).theme, theme);
    }
    w.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } });
    ready(&w);
    for panel in [Panel::Navigator, Panel::Stats, Panel::Layers] {
        w.customize(CustomizationAction::SetPanelVisible { panel, visible: false });
    }
    if std::env::var("LAYER_MOTION_VIEWPORT").is_ok_and(|size| size == "640x480") {
        for panel in Panel::ALL.into_iter().filter(|panel| !matches!(panel, Panel::Toolbar | Panel::Commands | Panel::Properties)) {
            w.customize(CustomizationAction::SetPanelVisible { panel, visible: false });
        }
    }
    w.customize(CustomizationAction::SetPanelVisible { panel: Panel::Properties, visible: true });
    let group = state(&w).workspace.layout.panel_group(Panel::Properties).unwrap();
    if state(&w).workspace.layout.select_tab(group, Panel::Properties).unwrap() {
        w.dispatch(UiAction::SelectPanelTab { group, panel: Panel::Properties });
    }
    assert_eq!(state(&w).customization.expanded, None);
    pump(250);
    let mut native = super::canvas_bar_tests::remote_input();
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let key = "curve_0";
    set(&w, "domain", EffectValue::Choice(0));
    set(&w, key, EffectValue::Curve(vec![[0., 0.], [0.5, 0.5], [1., 1.]]));
    let graph = curve_graph(&w, key);
    assert!(graph.is_mapped());
    assert!(graph.height() <= 210, "graph keeps its bounded 200-pixel layout: {}", graph.height());
    let before = snapshot(&w);
    native.click(screen_point(&graph, &w.window, [0.5, 0.5]));
    assert_eq!(state(&w).layer_properties.controls.iter().find(|c| c.key == key).unwrap().curve.as_ref().unwrap().selected, Some(1));
    assert_eq!(curve_points(&w, key), vec![[0., 0.], [0.5, 0.5], [1., 1.]], "zero-motion selection retains exact knots");
    let current = layer_core::Project::read(std::io::Cursor::new(snapshot(&w)), Default::default()).unwrap();
    let mut original = layer_core::Project::read(std::io::Cursor::new(before), Default::default()).unwrap();
    original.document.revision = current.document.revision;
    assert_eq!(current.document.layers, original.document.layers, "point selection does not change layer values");
    assert_eq!(current.document.sdr_rendition, original.document.sdr_rendition, "point selection does not change rendition");
    assert!(current == original, "point selection does not change stored values");
    for axis in ["input", "output"] {
        let control = state(&w).layer_properties.controls.into_iter().find(|c| c.key == key).unwrap().curve.unwrap();
        let expected = if axis == "input" { control.input.unwrap() } else { control.output.unwrap() };
        assert_eq!(curve_spin(&w, key, axis).text().as_str(), expected.text);
    }
    set(&w, key, EffectValue::Curve(vec![[0., 0.], [1., 1.]]));
    ready(&w);
    let middle = screen_point(&graph, &w.window, [0.5, 0.5]);
    native.perform(json!([{"point":middle,"down":true},{"down":false},{"wait_ms":80},{"down":true},{"down":false}]));
    let inserted = curve_points(&w, key);
    assert_eq!(inserted.len(), 3, "double-clicking empty graph leaves one inserted knot");
    assert_eq!(inserted[0], [0., 0.]);
    assert_eq!(inserted[2], [1., 1.]);
    set(&w, key, EffectValue::Curve(vec![[0., 0.], [0.5, 0.5], [1., 1.]]));
    ready(&w);
    capture_ui(&w, &output, "curves-encoded.png");
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { native.perform(json!([{"wait_ms":250},{"capture":"curves-encoded"}])); }
    pump(600);
    let from = screen_point(&graph, &w.window, [0.5, 0.5]);
    let to = screen_point(&graph, &w.window, [0.6, 0.35]);
    let checkpoint = ui_session(&w).engine().checkpoint();
    native.perform(json!([{"point":from,"down":true},{"wait_ms":40},{"point":to}]));
    assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint, "drag preview stays outside history");
    assert!(curve_points(&w, key)[1][1] > 0.6, "native graph drag: {:?}", curve_points(&w, key));
    native.perform(json!([{"down":false}]));
    let dragged = curve_points(&w, key);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo }); ready(&w);
    assert_eq!(curve_points(&w, key), vec![[0., 0.], [0.5, 0.5], [1., 1.]]);
    w.dispatch(UiAction::Invoke { command: CommandId::Redo }); ready(&w);
    assert_eq!(curve_points(&w, key), dragged);

    let spin = curve_spin(&w, key, "output");
    spin.grab_focus(); pump(50);
    let initial = curve_points(&w, key);
    let checkpoint = ui_session(&w).engine().checkpoint();
    native.perform(json!([{"key":0xff52,"down":true},{"wait_ms":600}]));
    assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
    assert_ne!(curve_points(&w, key), initial);
    native.perform(json!([{"key":0xff51,"down":false}]));
    assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint, "unmatched release does not commit");
    native.perform(json!([{"key":0xff52,"down":false}]));
    let repeated = curve_points(&w, key);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo }); ready(&w);
    assert_eq!(curve_points(&w, key), initial, "a held numeric key has one undo");
    w.dispatch(UiAction::Invoke { command: CommandId::Redo }); ready(&w);
    assert_eq!(curve_points(&w, key), repeated);

    spin.grab_focus(); pump(30);
    let initial = curve_points(&w, key);
    native.perform(json!([{"key":0xff54,"down":true}]));
    native.key(0xff1b);
    native.perform(json!([{"key":0xff54,"down":false}]));
    assert_eq!(curve_points(&w, key), initial, "Escape restores the gesture before native release");
    assert!(!state(&w).host_error.is_some());

    spin.grab_focus(); pump(30);
    let initial = curve_points(&w, key);
    native.perform(json!([{"key":0xff52,"down":true}]));
    let first = curve_points(&w, key);
    native.perform(json!([{"key":0xff54,"down":true}]));
    native.perform(json!([{"key":0xff52,"down":false},{"key":0xff54,"down":false}]));
    w.dispatch(UiAction::Invoke { command: CommandId::Undo }); ready(&w);
    assert_eq!(curve_points(&w, key), first, "key change commits the preceding key gesture");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo }); ready(&w);
    assert_eq!(curve_points(&w, key), initial);
    w.dispatch(UiAction::Invoke { command: CommandId::Redo });
    w.dispatch(UiAction::Invoke { command: CommandId::Redo }); ready(&w);

    let spin = curve_spin(&w, key, "output");
    spin.grab_focus(); pump(30);
    let initial = curve_points(&w, key);
    native.perform(json!([{"key":0xff52,"down":true}]));
    let accepted = curve_points(&w, key);
    assert_ne!(accepted, initial);
    let input_spin = curve_spin(&w, key, "input");
    native.perform(json!([{"point":screen_point(descendant::<gtk::Text>(&input_spin).unwrap().upcast_ref(),&w.window,[0.5,0.5]),"down":true},{"key":0xff52,"down":false},{"down":false}]));
    assert!(input_spin.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN));
    assert_eq!(curve_points(&w, key), accepted, "numeric focus loss commits before a later release");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo }); ready(&w);
    assert_eq!(curve_points(&w, key), initial, "numeric focus loss has one undo");

    let spin=curve_spin(&w,key,"output");
    spin.grab_focus();pump(30);
    let initial=curve_points(&w,key);
    native.perform(json!([{"key":0xff52,"down":true}]));
    let accepted=curve_points(&w,key);
    let input_spin=curve_spin(&w,key,"input");
    let input_buttons=descendants::<gtk::Button>(input_spin.upcast_ref());
    let increment=input_buttons.last().unwrap();
    native.perform(json!([{"point":screen_point(increment.upcast_ref(),&w.window,[0.5,0.5]),"down":true},{"key":0xff52,"down":false},{"down":false}]));
    let changed=curve_points(&w,key);
    assert_ne!(changed[1][0],accepted[1][0],"native Input step starts after held Output is retired");
    assert_eq!(changed[1][1],accepted[1][1]);
    w.dispatch(UiAction::Invoke{command:CommandId::Undo});ready(&w);
    assert_eq!(curve_points(&w,key),accepted,"step has its own undo");
    w.dispatch(UiAction::Invoke{command:CommandId::Undo});ready(&w);
    assert_eq!(curve_points(&w,key),initial,"prior held key has its own undo");

    let spin = curve_spin(&w, key, "output");
    let buttons = descendants::<gtk::Button>(spin.upcast_ref());
    for increment in [buttons.last().unwrap(), buttons.first().unwrap()] {
    let initial = curve_points(&w, key);
    let checkpoint = ui_session(&w).engine().checkpoint();
    native.perform(json!([{"point":screen_point(increment.upcast_ref(),&w.window,[0.5,0.5]),"down":true}]));
    assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
    native.perform(json!([{"down":false}]));
    assert_ne!(curve_points(&w, key), initial, "native spin increment edits on release");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo }); ready(&w);
    assert_eq!(curve_points(&w, key), initial, "release update belongs to the same gesture");
    }

    let graph = curve_graph(&w, key);
    let middle = curve_points(&w, key)[1];
    let point = screen_point(&graph, &w.window, [middle[0], 1.-middle[1]]);
    native.perform(json!([{"point":point},{"button":273,"down":true},{"button":273,"down":false}]));
    assert_eq!(curve_points(&w, key).len(), 2, "right-click removes the interior point");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo }); ready(&w);
    let graph = curve_graph(&w, key);
    let middle = curve_points(&w, key)[1];
    let point = screen_point(&graph, &w.window, [middle[0], 1.-middle[1]]);
    native.perform(json!([{"point":point,"down":true},{"down":false},{"down":true},{"down":false}]));
    assert_eq!(curve_points(&w, key).len(), 2, "double-click removes the interior point");
    pump(600);
    native.click(screen_point(&graph, &w.window, [0.5, 0.5]));
    assert_eq!(curve_points(&w, key).len(), 3, "a graph click inserts a point");
    graph.grab_focus(); pump(30);
    let initial = curve_points(&w, key);
    let checkpoint = ui_session(&w).engine().checkpoint();
    native.perform(json!([{"key":0xff52,"down":true},{"wait_ms":600}]));
    assert_ne!(curve_points(&w, key), initial);
    assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
    native.key(0xff1b); native.perform(json!([{"key":0xff52,"down":false}]));
    assert_eq!(curve_points(&w, key), initial, "graph Escape retires the held key");
    graph.grab_focus(); pump(30);
    native.perform(json!([{"key":0xff52,"down":true}]));
    curve_spin(&w, key, "output").grab_focus(); pump(50);
    native.perform(json!([{"key":0xff52,"down":false}]));
    assert_eq!(curve_points(&w, key), initial, "graph focus loss cancels the held key");
    graph.grab_focus(); pump(50);
    let initial = curve_points(&w, key);
    native.perform(json!([{"key":0xff52,"down":true},{"key":0xffe3,"down":true},{"key":0xff52,"down":false},{"key":0xffe3,"down":false}]));
    w.dispatch(UiAction::Invoke { command: CommandId::Undo }); ready(&w);
    assert_eq!(curve_points(&w, key), initial, "modifier change does not lose the held graph key release");
    graph.grab_focus(); pump(50);
    assert!(graph.has_focus());
    assert_eq!(state(&w).layer_properties.controls.iter().find(|c| c.key == key).unwrap().curve.as_ref().unwrap().selected, Some(1));
    native.key(0xffff);
    assert_eq!(curve_points(&w, key).len(), 2, "native Delete removes the selected knot");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo }); ready(&w);
    assert_eq!(curve_points(&w, key), initial);

    let page = named::<gtk::DropDown>(w.window.upcast_ref(), "properties-page");
    let before = snapshot(&w);
    choose_curve_option(&w, &mut native, &page, 1);
    assert_eq!(state(&w).layer_properties.page.as_deref(), Some("red"));
    assert_eq!(snapshot(&w), before, "page navigation is transient");
    let red = curve_graph(&w, "curve_1");
    native.click(screen_point(&red, &w.window, [0.3, 0.4]));
    assert_eq!(curve_points(&w, "curve_1").len(), 3);
    assert_eq!(curve_points(&w, key).len(), 3);
    capture_ui(&w, &output, "curves-red.png");
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { native.perform(json!([{"wait_ms":250},{"capture":"curves-red"}])); }
    choose_curve_option(&w, &mut native, &page, 0);
    assert_eq!(state(&w).layer_properties.page.as_deref(), Some("rgb"));

    set(&w, key, EffectValue::Curve(vec![[0., 0.], [0.5, 0.12345679], [1., 1.]]));
    let graph = curve_graph(&w, key);
    native.click(screen_point(&graph, &w.window, [0.5, 1.-0.12345679]));
    let unchanged = curve_points(&w, key);
    let checkpoint = ui_session(&w).engine().checkpoint();
    let spin = curve_spin(&w, key, "output");
    spin.grab_focus(); native.key(0xff0d); graph.grab_focus(); pump(50);
    assert_eq!(curve_points(&w, key), unchanged, "Enter and focus loss preserve the precise knot behind rounded Encoded text");
    assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
    assert_curve_readout_visible(&spin);
    capture_ui(&w, &output, "curves-encoded-precise.png");
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { native.perform(json!([{"wait_ms":250},{"capture":"curves-encoded-precise"}])); }

    let domain = named::<gtk::DropDown>(w.window.upcast_ref(), "property-domain");
    choose_curve_option(&w, &mut native, &domain, 1);
    assert_eq!(value(&w, "domain"), EffectValue::Choice(1));
    let graph = curve_graph(&w, key);
    let middle = curve_points(&w, key)[1];
    native.click(screen_point(&graph, &w.window, [middle[0], 1.-middle[1]]));
    let spin = curve_spin(&w, key, "output");
    for literal in ["1e-20", "8", "0"] {
        spin.grab_focus();
        native.perform(json!([{"key":0xffe3,"down":true},{"key":97,"down":true},{"key":97,"down":false},{"key":0xffe3,"down":false}]));
        for c in literal.chars() { native.key(c as u32); }
        native.key(0xff0d);
        let control = state(&w).layer_properties.controls.into_iter().find(|c| c.key == key).unwrap();
        let coordinate = control.curve.as_ref().unwrap().output.as_ref().unwrap();
        let wanted = literal.parse::<f64>().unwrap();
        assert!(if wanted == 0. { coordinate.value == 0. } else { (coordinate.value / wanted - 1.).abs() < 1e-5 }, "exact HDR field {literal}: {}", coordinate.text);
        assert_eq!(spin.text().as_str(), coordinate.text);
        if literal != "0" && std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
            native.perform(json!([{"wait_ms":250},{"capture":if literal == "8" { "curves-hdr-eight" } else { "curves-hdr-tiny" }}]));
        }
    }
    capture_ui(&w, &output, "curves-hdr.png");
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { native.perform(json!([{"wait_ms":250},{"capture":"curves-hdr"}])); }
    let saved = snapshot(&w);
    let reopened = layer_core::Project::read(std::io::Cursor::new(&saved), Default::default()).unwrap();
    assert_eq!(reopened.document.layers, ui_session(&w).engine().document().layers);
    native.finish();
    w.window.destroy(); pump(100);
}
