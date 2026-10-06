//! Tagged effect definitions through retained GTK controls and native archives.
use super::new_photo::{capture_ui, ready, response};
use super::place_source::authored_snapshot as snapshot;
use super::*;
use layer_core::{
    EffectValue, GradientStop,
    color::{DocumentColor, SampleDepth, RgbColor, RgbSpace},
};
use crate::color_editor::tests::{every_form, form};
use layer_ui::{ColorAction, ColorForm, EffectAction};
use serde_json::json;

fn press(w: &Rc<Workspace>, name: &str) {
    find_named(w.window.upcast_ref(), name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
    pump(100);
}
fn field(w: &Rc<Workspace>, form_name: ColorForm, text: &str) {
    form(w, 0, form_name);
    crate::color_editor::tests::value(w, 0, 0, text);
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
    let EffectValue::Curve(points) = active_effect(document).value(key).unwrap() else { panic!("curve property") };
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
pub(super) fn choose_curve_option(w: &Rc<Workspace>, native: &mut RemoteInput, drop: &gtk::DropDown, index: u32) {
    let point = curve_option_point(w, native, drop, index);
    native.click(point);
    assert_eq!(drop.selected(), index);
}
fn curve_option_point(w: &Rc<Workspace>, native: &mut RemoteInput, drop: &gtk::DropDown, index: u32) -> [f32; 2] {
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
    let item = drop.model().unwrap().item(index).unwrap();
    let text = if let Some(item) = item.downcast_ref::<gtk::StringObject>() {
        item.string().to_string()
    } else {
        item.downcast_ref::<gtk::glib::BoxedAnyObject>().unwrap().borrow::<(String, String)>().1.clone()
    };
    let option = widgets(drop.upcast_ref()).find(|widget| widget.is_mapped()
        && widget.native().is_some_and(|native| native.is::<gtk::Popover>())
        && widget.downcast_ref::<gtk::Label>().is_some_and(|label| label.text() == text))
        .unwrap_or_else(|| panic!("native choice {text} is visible"));
    screen_point(&option, &w.window, [0.5, 0.5])
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
        let alpha=f64::from(a[3])*(1.-t)+f64::from(b[3])*t;
        let rgba: [f64;4]=std::array::from_fn(|c|if c==3 {alpha} else {(f64::from(a[c])*f64::from(a[3])*(1.-t)+f64::from(b[c])*f64::from(b[3])*t)/alpha});
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
    composition_mut(&mut project).color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let original =
        RgbColor::new(RgbSpace::DisplayP3, [0.95, 0.12, 0.234567, 123. / 65535.]).unwrap();
    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: "photo_filter".into(),
        },
    });
    let bucket = find_named(w.window.upcast_ref(), "color-bucket").expect("each color parameter has its bucket");
    let label = bucket.parent().and_then(|line| line.parent()).and_then(|row| row.first_child());
    assert_eq!(label.and_downcast::<gtk::Label>().map(|l| l.text()).as_deref(), Some("Color"));
    w.dispatch(UiAction::SetColor { rgba: [0.2, 0.5, 0.1, 1.] });
    bucket.downcast::<gtk::Button>().unwrap().emit_clicked();
    ready(&w);
    assert_eq!(value(&w, "color"), EffectValue::Color(state(&w).colors.definition()));
    set(&w, "color", EffectValue::Color(original));
    let before = snapshot(&w);
    press(&w, "effect-color-color");
    every_form(&w);
    response(&w, "apply");
    ready(&w);
    assert_eq!(value(&w, "color"), EffectValue::Color(original));
    assert!(snapshot(&w) == before, "untouched formats create no edit");
    press(&w, "effect-color-color");
    field(&w, ColorForm::Rgb, "NaN");
    assert!(!crate::color_editor::tests::editor(&w).apply_button.is_sensitive());
    response(&w, "cancel");
    assert_eq!(snapshot(&w), before);
    press(&w, "effect-color-color");
    field(&w, ColorForm::RgbUnit, "0.1234567");
    response(&w, "apply");
    ready(&w);
    let edited = value(&w, "color");
    assert!(
        matches!(&edited, EffectValue::Color(c) if c.space == RgbSpace::ProPhoto && c.rgba[0] == 0.1234567 && c.rgba[3] == original.rgba[3])
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    ready(&w);
    let current =
        open_native_document(std::io::Cursor::new(snapshot(&w)));
    let previous =
        open_native_document(std::io::Cursor::new(&before));
    assert_eq!(
        artwork_manifest(&current), artwork_manifest(&previous),
        "undo restores all stored state; revision remains monotonic"
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    ready(&w);
    assert_eq!(value(&w, "color"), edited);

    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: "gradient_map".into(),
        },
    });
    let stops = layer_core::GradientDefinition {interpolation:layer_core::ColorMixSpace::Classic,stops:vec![
        GradientStop {
            position: 0.,
            color: original,
        },
        GradientStop {
            position: 1.,
            color: RgbColor::new(RgbSpace::AdobeRgb, [0.15, 0.6, 0.9, 0.8]).unwrap(),
        },
    ]};
    set(&w, "gradient", EffectValue::Gradient(stops.clone()));
    press(&w, "effect-gradient-color");
    field(&w, ColorForm::RgbUnit, "0.37");
    response(&w, "apply");
    ready(&w);
    let EffectValue::Gradient(mut accepted) = value(&w, "gradient") else {
        panic!()
    };
    let edited = accepted.stops[0].color;
    assert!(edited.space == RgbSpace::ProPhoto && edited.rgba[0] == 0.37 && edited.rgba[3] == original.rgba[3], "{edited:?}");
    assert_eq!(accepted.stops[1], stops.stops[1]);
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
    assert_gradient_pixels(&w, &bar, &accepted.stops);
    let mut input=RemoteInput::new().timeout_secs(30);input.ready();
    input.click(screen_point(&bar,&w.window,[0.5,0.5]));
    ready(&w);
    let EffectValue::Gradient(inserted)=value(&w,"gradient") else {panic!("gradient control")};
    assert_eq!(inserted.stops.len(),3);let position=inserted.stops[1].position;assert!((position-0.5).abs()<0.01);
    accepted.stops.insert(1,GradientStop {position,color:accepted.sample(position,RgbSpace::ProPhoto).unwrap()});input.finish();
    assert_eq!(
        value(&w, "gradient"),
        EffectValue::Gradient(accepted.clone())
    );
    let saved = snapshot(&w);
    let reopened = Workspace::with_project(
        &app,
        Some((
            open_native_document(std::io::Cursor::new(saved.clone())),
            None,
        )),
    );
    reopened.window.present();
    ready(&reopened);
    let layer = {
        let session=ui_session(&reopened);
        let scene=session.engine().document().scene();
        *scene.order().iter().find(|handle|scene.effect(**handle).is_some_and(|e|e.program.id.as_ref()=="gradient_map")).unwrap()
    };
    reopened.dispatch(UiAction::SelectLayer {id:layer_ui::occurrence_token(layer)});ready(&reopened);
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
    controls.append(&w.customization.icon(&w, "colors", 32));
    let window = gtk::Window::builder()
        .application(&*app)
        .child(&controls)
        .build();
    window.present();
    pump(100);
    w.color.widget.emit_clicked();
    pump(100);
    every_form(&w);
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
    let key = "rgb";
    set(&w, "domain", EffectValue::Choice(0));
    set(&w, key, EffectValue::Curve(vec![[0., 0.], [0.5, 0.5], [1., 1.]]));
    let graph = curve_graph(&w, key);
    assert!(graph.is_mapped());
    assert!(graph.height() <= 210, "graph keeps its bounded 200-pixel layout: {}", graph.height());
    let before = snapshot(&w);
    native.click(screen_point(&graph, &w.window, [0.5, 0.5]));
    assert_eq!(state(&w).layer_properties.controls.iter().find(|c| c.key == key).unwrap().curve.as_ref().unwrap().selected, Some(1));
    assert_eq!(curve_points(&w, key), vec![[0., 0.], [0.5, 0.5], [1., 1.]], "zero-motion selection retains exact knots");
    let current = open_native_document(std::io::Cursor::new(snapshot(&w)));
    let original = open_native_document(std::io::Cursor::new(before));
    assert_eq!(artwork_manifest(&current), artwork_manifest(&original), "point selection does not change layer values");
    assert_eq!(current.output().sdr, original.output().sdr, "point selection does not change rendition");
    assert_live_artwork_eq(&current, &original);
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
    let red = curve_graph(&w, "red");
    native.click(screen_point(&red, &w.window, [0.3, 0.4]));
    assert_eq!(curve_points(&w, "red").len(), 3);
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
    let reopened = open_native_document(std::io::Cursor::new(&saved));
    assert_live_artwork_eq(&reopened, ui_session(&w).engine().document());
    native.finish();
    w.window.destroy(); pump(100);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_gradient_editor_modes_contacts_and_archive() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app=native_test_app("art.capycanvas.GradientEditor");
    let output=std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(||std::path::PathBuf::from(artifact_dir("../../artifacts/photo-editing-color/p28-gtk")));
    std::fs::create_dir_all(&output).unwrap();
    let mut native=RemoteInput::new().timeout_secs(30);native.ready();
    for (theme,scheme) in [("light",adw::ColorScheme::ForceLight),("dark",adw::ColorScheme::ForceDark)] {
        app.style_manager().set_color_scheme(scheme);
        for effect in ["gradient_fill","gradient_map"] {
            let mut project=new_drawing(256,256,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
            paint_at_mut(&mut project,0).base=Some(layer_core::PaintBase::new((layer_core::color::source::rgba8_source([256,256],|x,y|[x as u8,y as u8,(255-x) as u8,255])).into()));
            let w=Workspace::with_project(&app,Some((project,None)));w.window.maximize();w.window.present();ready(&w);
            super::pointwise::configure_properties(&w);
            w.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:effect.into()}});ready(&w);
            let original=layer_core::GradientDefinition::new(vec![
                GradientStop {position:0.,color:RgbColor::new(RgbSpace::Srgb,[0.9,0.08,0.2,0.35]).unwrap()},
                GradientStop {position:1.,color:RgbColor::new(RgbSpace::DisplayP3,[0.05,0.65,0.95,1.]).unwrap()},
            ]);
            set(&w,"gradient",EffectValue::Gradient(original.clone()));
            capture_ui(&w,&output,&format!("{effect}-{theme}-initial.png"));
            let bar=named::<gtk::Widget>(w.window.upcast_ref(),"effect-gradient");
            if effect=="gradient_fill" {
                let controls=state(&w).layer_properties.controls;
                assert_eq!(controls.first().unwrap().key,"style","Shape is the first Fill control");
                assert_eq!(controls.first().unwrap().label,w.localization().text(layer_ui::localization::MessageId::RESOURCES_PARAMETER_GRADIENT_FILL_STYLE).to_string());
                let shape=named::<gtk::DropDown>(w.window.upcast_ref(),"property-style");
                assert!(shape.compute_bounds(&w.window).unwrap().y()<bar.compute_bounds(&w.window).unwrap().y(),"Shape appears above the gradient editor");
            }
            super::histogram::scroll_to(&bar);pump(100);
            let middle=screen_point(&bar,&w.window,[0.5,0.5]);native.click(middle);ready(&w);
            let EffectValue::Gradient(inserted)=value(&w,"gradient") else {panic!("gradient")};
            assert_eq!(inserted.stops.len(),3);assert!((inserted.stops[1].position-0.5).abs()<0.01);
            w.dispatch(UiAction::Color {action:ColorAction::Select {slot:layer_ui::ColorSlot::Background}});ready(&w);
            let bucket=named::<gtk::Button>(w.window.upcast_ref(),"effect-gradient-use-color");
            native.click(screen_point(bucket.upcast_ref(),&w.window,[0.5,0.5]));ready(&w);
            let mut colored=inserted.clone();colored.stops[1].color=state(&w).colors.definition();
            assert_eq!(value(&w,"gradient"),EffectValue::Gradient(colored),"bucket uses selected Background color on selected stop only");
            w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"gradient"),EffectValue::Gradient(inserted.clone()));
            w.dispatch(UiAction::Color {action:ColorAction::Select {slot:layer_ui::ColorSlot::Foreground}});ready(&w);
            let to=screen_point(&bar,&w.window,[0.65,0.5]);
            native.perform(json!([{"point":middle,"down":true},{"point":to},{"down":false}]));ready(&w);
            let EffectValue::Gradient(moved)=value(&w,"gradient") else {panic!("gradient")};
            assert!((moved.stops[1].position-0.65).abs()<0.02);
            w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"gradient"),EffectValue::Gradient(inserted.clone()));
            w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(value(&w,"gradient"),EffectValue::Gradient(moved.clone()));
            native.perform(json!([{"point":to,"down":true},{"point":middle}]));native.key(0xff1b);native.perform(json!([{"down":false}]));ready(&w);
            assert_eq!(value(&w,"gradient"),EffectValue::Gradient(moved));
            let interpolation=named::<gtk::DropDown>(w.window.upcast_ref(),"gradient-interpolation");
            for (index,mode) in [layer_core::ColorMixSpace::Oklab,layer_core::ColorMixSpace::LinearRgb,layer_core::ColorMixSpace::Classic].into_iter().enumerate() {
                choose_curve_option(&w,&mut native,&interpolation,index as u32);ready(&w);
                let EffectValue::Gradient(g)=value(&w,"gradient") else {panic!("gradient")};assert_eq!(g.interpolation,mode);
            }
            assert!(find_named(w.window.upcast_ref(),"gradient-dither").is_none(),"gradient dithering is always enabled without a control");
            native.click(screen_point(&bar,&w.window,[0.65,0.5]));ready(&w);
            let number=named::<crate::number_control::NumberControl>(w.window.upcast_ref(),"effect-gradient-position");
            super::histogram::scroll_to(number.upcast_ref());
            if let Some(spin)=descendant::<gtk::SpinButton>(&number) {
                native.click(screen_point(spin.upcast_ref(),&w.window,[0.4,0.5]));spin.set_text("43");
            } else {
                let display=find_css(number.upcast_ref(),"number-value").unwrap();native.click(screen_point(&display,&w.window,[0.5,0.5]));
                let entry=descendant::<gtk::Entry>(&number).unwrap();entry.set_text("43");
            }
            native.key(0xff0d);ready(&w);
            let EffectValue::Gradient(g)=value(&w,"gradient") else {panic!("gradient")};assert!((g.stops[1].position-0.43).abs()<0.0001);
            native.click(screen_point(&bar,&w.window,[0.43,0.5]));
            native.perform(json!([{"key":0xff53,"down":true},{"wait_ms":400},{"key":0xff53,"down":false}]));ready(&w);
            let EffectValue::Gradient(stepped)=value(&w,"gradient") else {panic!("gradient")};assert!(stepped.stops[1].position>g.stops[1].position);
            w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"gradient"),EffectValue::Gradient(g.clone()));
            native.perform(json!([{"key":0xff53,"down":true},{"wait_ms":200}]));native.key(0xff1b);native.perform(json!([{"key":0xff53,"down":false}]));ready(&w);
            assert_eq!(value(&w,"gradient"),EffectValue::Gradient(g.clone()));
            let color=named::<gtk::Button>(w.window.upcast_ref(),"effect-gradient-color");
            native.click(screen_point(color.upcast_ref(),&w.window,[0.5,0.5]));field(&w,ColorForm::RgbUnit,"0.42");response(&w,"cancel");ready(&w);
            assert_eq!(value(&w,"gradient"),EffectValue::Gradient(g.clone()));
            native.click(screen_point(color.upcast_ref(),&w.window,[0.5,0.5]));field(&w,ColorForm::RgbUnit,"0.42");response(&w,"apply");ready(&w);
            let EffectValue::Gradient(colored)=value(&w,"gradient") else {panic!("gradient")};
            assert!((colored.stops[1].color.rgba[0]-0.42).abs()<0.0001);
            assert_eq!(colored.stops[1].color.rgba[3],g.stops[1].color.rgba[3]);
            w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"gradient"),EffectValue::Gradient(g));
            w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(value(&w,"gradient"),EffectValue::Gradient(colored.clone()));
            native.click(screen_point(color.upcast_ref(),&w.window,[0.5,0.5]));
            field(&w,ColorForm::LinearRgb,"0.25");response(&w,"apply");ready(&w);
            let EffectValue::Gradient(hdr)=value(&w,"gradient") else {panic!("gradient")};assert!((hdr.stops[1].color.linear_in(RgbSpace::Srgb).unwrap()[0]-0.25).abs()<0.00001);
            assert_eq!(hdr.stops[1].color.rgba[3],colored.stops[1].color.rgba[3]);
            w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"gradient"),EffectValue::Gradient(colored));
            w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(value(&w,"gradient"),EffectValue::Gradient(hdr));
            if std::env::var_os("LAYER_NATIVE_INPUT_TRACE").is_some() {
                gradient_motion(&w,&mut native,screen_point(&bar,&w.window,[0.43,0.5]),screen_point(&bar,&w.window,[0.75,0.5]),&output,&format!("{effect}-{theme}-stop"),true);
            }
            let properties=w.panel_widget(Panel::Properties);
            let scroller=w.effects.properties.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
            let group=w.groups.borrow().iter().find(|group|group.panels.contains(&Panel::Properties) && group.root.is_mapped()).unwrap().root.clone();
            let visible=scroller.compute_bounds(&group).unwrap();
            assert!(visible.x()>=-1. && visible.x()+visible.width()<=group.width() as f32+1.,"Properties viewport fits its visible dock");
            for name in ["effect-gradient","effect-gradient-position","effect-gradient-color","gradient-interpolation"] {
                let widget=find_named(w.window.upcast_ref(),name).unwrap();let bounds=widget.compute_bounds(&properties).unwrap();
                assert!(bounds.x()>=-1. && bounds.x()+bounds.width()<=properties.width() as f32+1.,"{name} fits Properties: {bounds:?}");
                let visible=widget.compute_bounds(&scroller).unwrap();
                assert!(visible.x()>=-1. && visible.x()+visible.width()<=scroller.width() as f32+1.,"{name} fits the actual Properties viewport: {visible:?}");
            }
            capture_ui(&w,&output,&format!("{effect}-{theme}.png"));
            let (saved,bytes)=super::pointwise::saved_artwork(&w);let expected=value(&w,"gradient");
            let owner=ui_session(&w).engine().document().working.occurrence.unwrap();let id=ui_session(&w).engine().document().artwork.occurrences.id(owner).unwrap();
            let project=open_native_document(std::io::Cursor::new(bytes));super::pointwise::assert_saved_artwork(&saved,&project);
            let reopened=Workspace::with_project(&app,Some((project,None)));
            reopened.window.maximize();reopened.window.present();ready(&reopened);
            let owner=ui_session(&reopened).engine().document().artwork.occurrences.resolve(id).unwrap();
            reopened.dispatch(UiAction::SelectLayer {id:layer_ui::occurrence_token(owner)});ready(&reopened);assert_eq!(value(&reopened,"gradient"),expected);
            super::pointwise::configure_properties(&reopened);
            capture_ui(&reopened,&output,&format!("{effect}-{theme}-reopened.png"));reopened.window.close();w.window.close();pump(100);
        }
    }
    native.finish();
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_gradient_tool_editor_shapes_and_reverse() {
    let app=native_test_app("art.capycanvas.GradientToolEditor");
    let output=std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(||std::path::PathBuf::from(artifact_dir("../../artifacts/photo-editing-color/p28-gtk")));
    std::fs::create_dir_all(&output).unwrap();
    let mut native=RemoteInput::new().timeout_secs(30);native.ready();
    for theme in [Theme::Light,Theme::Dark] {
        let project=new_drawing(256,256,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let w=Workspace::with_project(&app,Some((project,None)));w.window.maximize();w.window.present();ready(&w);
        super::pointwise::configure_properties(&w);w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        let properties=state(&w).layer_properties;
        w.dispatch(UiAction::Invoke {command:CommandId::Gradient});ready(&w);
        assert_eq!(state(&w).layer_properties,properties,"Gradient tool preserves selected layer Properties");
        w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Properties,visible:false});
        w.customize(CustomizationAction::SetPanelVisible {panel:Panel::ToolSettings,visible:true});
        w.dispatch(UiAction::MovePanel {panel:Panel::ToolSettings,target:DockTarget::Edge {edge:Edge::Right,outer:false},viewport:[w.window.width() as f32,800.]});ready(&w);
        let mut workspace=state(&w).workspace;
        let removed:Vec<_>=workspace.layout.panel(Panel::Commands).unwrap().tiles().iter().map(|tile|tile.id).collect();
        for id in removed {workspace.layout.remove_tool(Panel::Commands,id).unwrap();}
        workspace.layout.insert_tools(Panel::Commands,None,&[ToolbarControl::TOOL_OPTIONS]).unwrap();
        w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(workspace)});ready(&w);
        for index in [2,0,1,2,1,0] {
            let point={let choice=named::<gtk::DropDown>(w.window.upcast_ref(),"toolbar-choice-variant");
                assert!(choice.is_mapped());curve_option_point(&w,&mut native,&choice,index)};
            native.click(point);ready(&w);
            assert_eq!(state(&w).layer_tools.tool,LayerCanvasTool::Gradient {shape:layer_core::GradientShape::ALL[index as usize]});
        }
        let before=super::editing_tools::raster(&w);let original=super::editing_tools::pixels(&before);
        super::editing_tools::stroke(&mut native,&w,[32.,64.],[224.,192.]);super::editing_tools::committed(&w,&before);
        assert_ne!(super::editing_tools::pixels(&super::editing_tools::raster(&w)),original);
        w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(super::editing_tools::pixels(&super::editing_tools::raster(&w)),original);
        let opener=named::<gtk::MenuButton>(w.window.upcast_ref(),"toolbar-gradient");
        assert!(opener.is_mapped(),"horizontal Tool Options preview is mapped");
        capture_ui(&w,&output,&format!("tool-options-before-{theme:?}.png"));
        native.click(screen_point(opener.upcast_ref(),&w.window,[0.5,0.5]));pump(150);
        let popup=opener.popover().expect("tool options gradient editor");assert!(popup.is_visible());
        assert!(find_named(popup.upcast_ref(),"gradient-shape").is_none());assert!(find_named(popup.upcast_ref(),"gradient-dither").is_none());
        if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {native.perform(json!([{"wait_ms":150},{"capture":format!("tool-options-{}",format!("{theme:?}").to_lowercase())}]));}
        let reverse=named::<gtk::Button>(popup.upcast_ref(),"gradient-reverse");
        let EffectValue::Gradient(before)=tool_gradient_control(&w).value else {panic!("tool gradient")};
        let mut expected=before.clone();expected.reverse();
        native.click(screen_point(reverse.upcast_ref(),&w.window,[0.5,0.5]));ready(&w);
        assert_eq!(tool_gradient_control(&w).value,EffectValue::Gradient(expected),"Tool Options and ToolSettings share stop order");
        native.click(screen_point(reverse.upcast_ref(),&w.window,[0.5,0.5]));ready(&w);
        assert_eq!(tool_gradient_control(&w).value,EffectValue::Gradient(before));
        capture_ui(&w,&output,&format!("tool-options-{theme:?}.png"));popup.popdown();pump(200);
        for (index,shape) in layer_core::GradientShape::ALL.into_iter().enumerate() {
            let shape_button=named::<gtk::ToggleButton>(&w.panel_widget(Panel::ToolSettings),&format!("tool-choice-gradient-shape-{index}"));
            let bar=named::<gtk::Widget>(&w.panel_widget(Panel::ToolSettings),"effect-gradient");
            assert!(shape_button.compute_bounds(&w.window).unwrap().y()<bar.compute_bounds(&w.window).unwrap().y(),"shape precedes gradient editor");
            native.click(screen_point(shape_button.upcast_ref(),&w.window,[0.5,0.5]));ready(&w);
            assert_eq!(state(&w).layer_tools.tool,LayerCanvasTool::Gradient {shape});
            let reverse=named::<gtk::Button>(&w.panel_widget(Panel::ToolSettings),"gradient-reverse");
            for reversed in [false,true] {
                if reversed {
                    let EffectValue::Gradient(before)=tool_gradient_control(&w).value else {panic!("tool gradient")};
                    let mut expected=before.clone();expected.reverse();
                    native.click(screen_point(reverse.upcast_ref(),&w.window,[0.5,0.5]));ready(&w);
                    assert_eq!(tool_gradient_control(&w).value,EffectValue::Gradient(expected),"native Reverse mutates stop order and positions");
                }
                let before=super::editing_tools::raster(&w);let old_pixels=super::editing_tools::pixels(&before);
                if std::env::var_os("LAYER_NATIVE_INPUT_TRACE").is_some() && !reversed {
                    let m=state(&w).camera.document_to_surface();let scale=w.area.scale_factor() as f32;
                    let at=|p:[f32;2]| {let p=w.area.compute_point(&w.window,&gtk::graphene::Point::new((m[0]*p[0]+m[2]*p[1]+m[4])/scale,(m[1]*p[0]+m[3]*p[1]+m[5])/scale)).unwrap();[p.x(),p.y()]};
                    gradient_motion(&w,&mut native,at([32.,64.]),at([224.,192.]),&output,&format!("tool-{shape:?}-{theme:?}"),false);
                } else {super::editing_tools::stroke(&mut native,&w,[32.,64.],[224.,192.]);}
                super::editing_tools::committed(&w,&before);
                let after=super::editing_tools::raster(&w);let new_pixels=super::editing_tools::pixels(&after);assert_ne!(old_pixels,new_pixels);
                capture_ui(&w,&output,&format!("tool-{shape:?}-{reversed}-{theme:?}.png"));
                let position=named::<gtk::Widget>(&w.panel_widget(Panel::ToolSettings),"effect-gradient-position");
                let label=widgets(&position).filter_map(|widget|widget.downcast::<gtk::Label>().ok()).find(|label|label.has_css_class("number-readout")).unwrap();
                let layout=label.layout();
                let measurement=json!({"text":label.text().to_string(),"allocated_width":label.width(),"layout_pixels":layout.pixel_size(),"ellipsized":layout.is_ellipsized()});
                std::fs::write(output.join(format!("tool-position-{shape:?}-{reversed}-{theme:?}.json")),serde_json::to_vec_pretty(&measurement).unwrap()).unwrap();
                if index==0 && reversed {assert!(label.text().starts_with("100"),"selected final endpoint: {measurement}");}
                assert!(label.is_mapped() && !layout.is_ellipsized(),"complete gradient position readout: {measurement}");
                w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(super::editing_tools::pixels(&super::editing_tools::raster(&w)),old_pixels);
                w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(super::editing_tools::pixels(&super::editing_tools::raster(&w)),new_pixels);
                w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);
            }
        }
        w.dispatch(UiAction::SetBrushOpacity {value:0.6});ready(&w);
        let artwork=ui_session(&w).engine().document().working.target.unwrap();
        let original=super::editing_tools::pixels(&super::editing_tools::raster(&w));let colors=state(&w).colors.clone();let opacity=state(&w).brush.opacity;
        native.key(b'q' as u32);ready(&w);assert!(state(&w).layer_tools.quick_mask);
        w.dispatch(UiAction::Effect {action:EffectAction::Set {layer:0,key:"mask_mode".into(),value:EffectValue::Choice(1)}});ready(&w);
        w.dispatch(UiAction::Invoke {command:CommandId::Gradient});ready(&w);
        let target=tool_gradient_control(&w).gradient.as_ref().unwrap().destination.clone();
        for (index,gray) in [(0,1.),(1,0.)] {
            w.dispatch(UiAction::Effect {action:EffectAction::Gradient {target:target.clone(),edit:layer_ui::GradientEdit::Stop {index:Some(index),position:index as f32,color:Some(RgbColor::new(RgbSpace::Srgb,[gray,gray,gray,1.]).unwrap()),remove:false}}});
        }
        for edit in [layer_ui::GradientEdit::Interpolation {value:layer_core::ColorMixSpace::LinearRgb}] {
            w.dispatch(UiAction::Effect {action:EffectAction::Gradient {target:target.clone(),edit}});
        }
        w.dispatch(UiAction::SetBrushOpacity {value:0.6});ready(&w);
        let shape_action=state(&w).tool_extra.iter().find_map(|option|match option {layer_ui::ToolOption::Choice {id:"gradient-shape",items,..}=>Some(items[0].action.clone()),_=>None}).expect("shared Gradient shape action");
        w.dispatch(shape_action);ready(&w);
        assert_eq!(state(&w).layer_tools.tool,LayerCanvasTool::Gradient {shape:layer_core::GradientShape::Linear});
        let EffectValue::Gradient(configured)=tool_gradient_control(&w).value else {panic!("mask gradient")};assert_eq!(configured.interpolation,layer_core::ColorMixSpace::LinearRgb);
        assert_eq!(configured.stops[0].color.rgba,[1.,1.,1.,1.]);assert_eq!(configured.stops[1].color.rgba,[0.,0.,0.,1.]);
        capture_ui(&w,&output,&format!("tool-quick-mask-before-{theme:?}.png"));
        std::fs::write(output.join(format!("tool-quick-mask-state-{theme:?}.json")),serde_json::to_vec_pretty(&json!({"gradient":tool_gradient_control(&w).value,"controls":tool_gradient_control(&w).gradient,"camera":state(&w).camera,"opacity":state(&w).brush.opacity,"selection_painting":state(&w).settings.selection_painting,"area":[w.area.width(),w.area.height()]})).unwrap()).unwrap();
        super::editing_tools::stroke(&mut native,&w,[32.,64.],[224.,192.]);
        let deadline=Instant::now()+Duration::from_secs(15);
        while ui_session(&w).engine().document().working.selection.is_none() {pump(10);assert!(Instant::now()<deadline,"native quick-mask gradient commits");}
        ready(&w);let selection=ui_session(&w).engine().document().working.selection.clone().unwrap();
        let layer_core::SelectionShape::Pixels(pixels)=&selection.shape else {panic!("gradient pixel coverage")};assert_eq!(pixels.coverage_format(),2);
        let at=|x:u32,y:u32| (pixels.words()[(y*pixels.extent()[0].div_ceil(4)+x/4) as usize]>>((x%4)*8))&255;
        std::fs::write(output.join(format!("tool-quick-mask-probes-{theme:?}.json")),serde_json::to_vec_pretty(&json!((0..8).map(|x|[x*32,at(x*32,128)]).collect::<Vec<_>>())).unwrap()).unwrap();
        for (x,y) in [(64,128),(192,128)] {
            let t=((x as f32-32.)*192.+(y as f32-64.)*128.)/(192.*192.+128.*128.);
            let expected=(255.*0.6*(1.-t)).round() as i32;assert!((at(x,y) as i32-expected).abs()<=2,"gray/opacity coverage at {x},{y}: {} vs {expected}",at(x,y));
        }
        assert_eq!(super::editing_tools::pixels(ui_session(&w).engine().document().target_raster(artwork).unwrap()),original);
        capture_ui(&w,&output,&format!("tool-quick-mask-{theme:?}.png"));
        w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert!(ui_session(&w).engine().document().working.selection.is_none());
        w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(ui_session(&w).engine().document().working.selection.as_ref(),Some(&selection));
        native.key(b'q' as u32);ready(&w);assert!(!state(&w).layer_tools.quick_mask);assert_eq!(state(&w).colors,colors);assert_eq!(state(&w).brush.opacity,opacity);
        w.dispatch(UiAction::Invoke {command:CommandId::SaveSelectionLayer});ready(&w);
        w.dispatch(UiAction::Layer {action:LayerAction::CancelRename});ready(&w);
        let saved=ui_session(&w).engine().document().working.occurrence.unwrap();let before=ui_session(&w).engine().document().saved_selection(saved).unwrap();
        w.dispatch(UiAction::Invoke {command:CommandId::Gradient});ready(&w);
        let reverse=named::<gtk::Button>(&w.panel_widget(Panel::ToolSettings),"gradient-reverse");native.click(screen_point(reverse.upcast_ref(),&w.window,[0.5,0.5]));ready(&w);
        super::editing_tools::stroke(&mut native,&w,[32.,64.],[224.,192.]);
        let deadline=Instant::now()+Duration::from_secs(15);
        while ui_session(&w).engine().document().saved_selection(saved).unwrap()==before {pump(10);assert!(Instant::now()<deadline,"saved-selection gradient commits");}
        ready(&w);let after=ui_session(&w).engine().document().saved_selection(saved).unwrap();
        assert_eq!(super::editing_tools::pixels(ui_session(&w).engine().document().target_raster(artwork).unwrap()),original);
        capture_ui(&w,&output,&format!("tool-saved-selection-{theme:?}.png"));
        w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(ui_session(&w).engine().document().saved_selection(saved).unwrap(),before);
        w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(ui_session(&w).engine().document().saved_selection(saved).unwrap(),after);
        w.window.close();pump(100);
    }
    native.finish();
}

fn tool_gradient_control(w:&Workspace)->layer_ui::PropertyControl {
    state(w).tool_extra.into_iter().find_map(|option|match option {layer_ui::ToolOption::Gradient(control)=>Some(*control),_=>None}).expect("shared tool gradient editor")
}

fn gradient_motion(w:&Rc<Workspace>,input:&mut RemoteInput,from:[f32;2],to:[f32;2],output:&std::path::Path,name:&str,artwork_changes:bool) {
    for contact in 0..3 {
    let name=format!("{name}-{contact}");let (from,to)=if contact%2==0 {(from,to)} else {(to,from)};
    let stats=ui_session(w).engine().backend().stats.clone();
    let deadline=Instant::now()+Duration::from_secs(10);
    while !ui_session(w).engine().backend().frames_idle() || w.frame_timer.borrow().is_some() {
        pump(10);assert!(Instant::now()<deadline,"gradient preceding frame settles");
    }
    let baseline=stats.lock().unwrap().camera_views.last().map(|entry|entry.2);*stats.lock().unwrap()=Default::default();
    let mut events=vec![json!({"point":from,"down":true})];
    for index in 1..=250 {
        let amount=(1.-(index as f32*std::f32::consts::TAU/125.).cos())*0.5;
        events.push(json!({"point":[from[0]+(to[0]-from[0])*amount,from[1]+(to[1]-from[1])*amount]}));
    }
    events.push(json!({"point":to}));events.push(json!({"down":false}));let step=input.step;input.perform(json!(events));ready(w);pump(200);
    let trace:serde_json::Value=serde_json::from_slice(&std::fs::read(input.dir.join(format!("trace-{step}.json"))).unwrap()).unwrap();
    let entries=trace.as_array().unwrap();let start=entries[1]["ns"].as_u64().unwrap();let end=entries[entries.len()-2]["ns"].as_u64().unwrap();
    assert!((5_000_000_000..=10_000_000_000).contains(&(end-start)),"sustained native motion requires 5–10 seconds: {}",(end-start) as f64/1e9);
    let mut report=super::photo_drop::frames(&stats);let data=stats.lock().unwrap();let mut revision=baseline;let mut moving=Vec::new();
    for frame in data.presented.iter().filter(|frame|frame[3]==1 && frame[1]>=start && frame[1]<=end) {
        if !artwork_changes {moving.push(*frame);} else if let Some(view)=data.camera_views.iter().find(|view|view.0==frame[0]) && Some(view.2)!=revision {moving.push(*frame);revision=Some(view.2);}
    }
    drop(data);let intervals:Vec<f64>=moving.windows(2).map(|pair|pair[1][1].saturating_sub(pair[0][1]) as f64/1e6).collect();
    report["motion_kind"]=json!(if artwork_changes {"stop edit with changed artwork revision"} else {"tool geometry overlay during isolated contact"});
    report["canvas"]=json!([256,256]);report["viewport"]=json!([w.window.width(),w.window.height()]);report["reference_tier_qualification"]=json!(false);
    report["input_trace"]=trace;report["motion_window_ns"]=json!([start,end]);report["moving_presentations"]=json!(moving);report["moving_presentation_intervals_ms"]=json!(intervals);
    report["moving_presentations_per_s"]=json!(moving.len() as f64/((end-start) as f64/1e9));assert!(!moving.is_empty(),"gradient moving presentations recorded");
    std::fs::write(output.join(format!("{name}-motion.json")),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    if !artwork_changes && contact<2 {w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(w);}
    }
}

#[test]
#[ignore]
fn native_gradient_canvas_band_diagnostics() {
    let app=native_test_app("art.capycanvas.GradientBandDiagnostics");
    let output=std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").expect("private band diagnostics output"));
    std::fs::create_dir_all(&output).unwrap();
    let mut native=RemoteInput::new().timeout_secs(30);native.ready();
    for theme in [Theme::Light,Theme::Dark] {for (label,a,b) in [("default",0.,1.),("light-shallow",0.70,0.75)] {
        let document=new_drawing_at(1024,512,SampleDepth::U8);
        let w=Workspace::with_project(&app,Some((document,None)));w.window.maximize();w.window.present();ready(&w);
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas});ready(&w);
        w.dispatch(UiAction::Invoke {command:CommandId::Gradient});ready(&w);
        let target=tool_gradient_control(&w).gradient.as_ref().unwrap().destination.clone();
        for (index,gray) in [(0,a),(1,b)] {
            w.dispatch(UiAction::Effect {action:EffectAction::Gradient {target:target.clone(),edit:layer_ui::GradientEdit::Stop {
                index:Some(index),position:index as f32,color:Some(RgbColor::new(RgbSpace::Srgb,[gray,gray,gray,1.]).unwrap()),remove:false}}});
        }
        ready(&w);
        let before=super::editing_tools::raster(&w);
        super::editing_tools::stroke(&mut native,&w,[32.,256.],[992.,256.]);
        super::editing_tools::committed(&w,&before);
        let raster=super::editing_tools::pixels(&super::editing_tools::raster(&w));
        let mut rgba=vec![0u8;1024*512*4];
        for (key,bytes) in raster {
            if key.plane!=layer_core::raster::RasterPlane::Color {continue;}
            for y in 0..256usize {
                let destination=((key.coordinate[1] as usize*256+y)*1024+key.coordinate[0] as usize*256)*4;
                rgba[destination..destination+1024].copy_from_slice(&bytes[y*1024..(y+1)*1024]);
            }
        }
        std::fs::write(output.join(format!("{label}-{theme:?}-committed-rgba8.bin")),rgba).unwrap();
        for (mode,zoom) in [("fit",None),("native",Some(1.)),("zoom",Some(2.))] {
            if let Some(zoom)=zoom {w.dispatch(UiAction::SetZoom {zoom});}else{w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas});}
            ready(&w);pump(300);
            capture_ui(&w,&output,&format!("{label}-{theme:?}-{mode}.png"));
            let m=state(&w).camera.document_to_surface();
            let area=w.area.compute_bounds(&w.window).unwrap();
            let metadata=json!({"source":[1024,512],"camera":m,"area":[area.x(),area.y(),area.width(),area.height()],"window":[w.window.width(),w.window.height()],"scale":w.area.scale_factor(),"gradient":tool_gradient_control(&w).value});
            std::fs::write(output.join(format!("{label}-{theme:?}-{mode}.json")),serde_json::to_vec_pretty(&metadata).unwrap()).unwrap();
        }
        w.window.close();pump(100);
    }}
}

#[test]
#[ignore = "private GTK display, deferred native color dialog and hardware GPU"]
fn native_gradient_color_completion_keeps_its_original_destination() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app=native_test_app("art.capycanvas.GradientDeferredOwner");
    let input=RemoteInput::new().timeout_secs(30);input.ready();
    for scheme in [adw::ColorScheme::ForceLight,adw::ColorScheme::ForceDark] {
        app.style_manager().set_color_scheme(scheme);
        let project=new_drawing(64,64,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let w=Workspace::with_project(&app,Some((project,None)));w.window.present();ready(&w);
        super::pointwise::configure_properties(&w);
        w.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"gradient_map".into()}});ready(&w);
        let old=state(&w).layer_properties.controls.into_iter().find(|c|c.key=="gradient").unwrap();
        let editor=crate::effects::GradientEditor::new(&w,&old);editor.update_control(&old);
        let retained=gtk::Window::new();retained.set_application(Some(&app.0));retained.set_child(Some(&editor.root));retained.present();pump(100);
        named::<gtk::Button>(retained.upcast_ref(),"effect-gradient-color").emit_clicked();
        until(|| w.window.visible_dialog().is_some_and(|dialog|dialog.widget_name()=="edit-color-dialog" && dialog.is_mapped()),"deferred gradient color dialog");
        field(&w,ColorForm::RgbUnit,"0.42");
        w.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"gradient_map".into()}});ready(&w);
        let replacement=state(&w).layer_properties.controls.into_iter().find(|c|c.key=="gradient").unwrap();
        assert_eq!(old.value,replacement.value);
        assert_ne!(old.gradient.as_ref().unwrap().destination,replacement.gradient.as_ref().unwrap().destination);
        editor.update_control(&replacement);
        let checkpoint=ui_session(&w).engine().checkpoint();
        response(&w,"apply");ready(&w);
        assert_eq!(value(&w,"gradient"),replacement.value,"deferred stop color cannot cross a retained editor destination");
        assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint,"stale completion creates no Undo entry");
        retained.close();w.window.close();pump(100);
    }
    input.finish();
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_opaque_filter_colors_keep_full_alpha_in_both_themes() {
    let app=native_test_app("art.capycanvas.OpaqueFilterColors");
    let w=fixture_workspace(&app);w.window.present();ready(&w);
    for theme in [layer_ui::Theme::Light,layer_ui::Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        w.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:"black_white".into()}});
        ready(&w);
        let color=RgbColor::new(RgbSpace::DisplayP3,[0.8,0.2,0.1,0.25]).unwrap();
        set(&w,"tint_color",EffectValue::Color(color));
        let EffectValue::Color(authored)=value(&w,"tint_color") else {panic!()};
        assert_eq!(authored.rgba[3],1.);
        press(&w,"effect-color-tint_color");
        every_form(&w);
        response(&w,"apply");ready(&w);
        assert_eq!(value(&w,"tint_color"),EffectValue::Color(authored));
        let saved=snapshot(&w);let reopened=open_native_document(std::io::Cursor::new(saved));
        let scene=reopened.scene();
        let effect=scene.order().iter().find_map(|handle|scene.effect(*handle).filter(|e|e.program.id.as_ref()=="black_white")).unwrap();
        assert_eq!(effect.value("tint_color"),Some(&EffectValue::Color(authored)));
    }
    w.window.close();
}
