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
    find_named(
        w.window.visible_dialog().unwrap().upcast_ref(),
        &format!("edit-color-value-{index}"),
    )
    .unwrap()
    .downcast::<adw::EntryRow>()
    .unwrap()
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
    let mut project = new_drawing(128, 128).unwrap();
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
