use super::new_photo::{finish, invoke, ready};
use super::*;
use layer_core::{
    Project,
    color::{histogram::Histogram, source::*, *},
};

fn fixture() -> Project {
    let mut p = new_drawing(64, 16).unwrap();
    p.document.color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    p.document
        .layers
        .iter_mut()
        .find(|l| l.kind == layer_core::LayerKind::Background)
        .unwrap()
        .visible = false;
    let mut source = SourceBuilder::new(
        [64, 16],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    let pixels: [[u16; 4]; 4] = [
        [0, 0, 0, 65535],
        [20000, 30000, 40000, 32768],
        [65535; 4],
        [60000, 30000, 10000, 0],
    ];
    let row: Vec<u8> = (0..64)
        .flat_map(|x| pixels[x / 16].into_iter().flat_map(u16::to_le_bytes))
        .collect();
    for _ in 0..16 {
        source.push_row(&row).unwrap();
    }
    p.document.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));
    let mut mask =
        layer_core::LayerMask::reveal_all(p.document.allocate_layer_id(), Point { x: 0., y: 0. });
    mask.default_coverage = 0.5;
    mask.show_area = true;
    p.document.layers[0].mask = Some(mask);
    p
}
fn label(inspector: &crate::histogram::Inspector, name: &str) -> gtk::Label {
    find_named(inspector.window.upcast_ref(), name)
        .unwrap()
        .downcast()
        .unwrap()
}
fn completed(inspector: &crate::histogram::Inspector) -> Histogram {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        pump(20);
        if label(inspector, "histogram-status")
            .text()
            .starts_with("Current")
        {
            return inspector.result.borrow().clone().unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "{}",
            label(inspector, "histogram-status").text()
        );
    }
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_composite_histogram_updates_without_changing_the_drawing() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.Histogram");
    let w = Workspace::with_project(&app, Some((fixture(), None)));
    w.window.present();
    ready(&w);
    let before = super::place_source::snapshot(&w);
    invoke(&w, CommandId::Histogram);
    finish(&w);
    let inspector = w.histogram.borrow().as_ref().unwrap().clone();
    assert!(!inspector.window.is_modal());
    let initial = completed(&inspector);
    find_named(inspector.window.upcast_ref(), "histogram-details").unwrap().downcast::<gtk::Expander>().unwrap().set_expanded(true);
    pump(100);
    let output = std::path::PathBuf::from(format!(
        "../../artifacts/color-m2/histogram-ui/{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&output).unwrap();
    pump(200);
    crate::snapshot_window(&inspector.window, 1.)
        .save_to_png(output.join("rgb.png"))
        .unwrap();
    let channel = find_named(inspector.window.upcast_ref(), "histogram-channel")
        .unwrap()
        .downcast::<gtk::DropDown>()
        .unwrap();
    channel.set_selected(4);
    pump(100);
    assert!(
        label(&inspector, "histogram-range")
            .text()
            .starts_with("Luminance:")
    );
    crate::snapshot_window(&inspector.window, 1.)
        .save_to_png(output.join("luminance.png"))
        .unwrap();
    assert_eq!(super::place_source::snapshot(&w), before);

    let automatic = find_named(inspector.window.upcast_ref(), "histogram-automatic")
        .unwrap()
        .downcast::<gtk::CheckButton>()
        .unwrap();
    automatic.set_active(false);
    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: "exposure".into(),
        },
    });
    ready(&w);
    let effect = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .active_layer;
    w.dispatch(UiAction::Effect {
        action: EffectAction::Set {
            layer: effect.0,
            key: "exposure".into(),
            value: layer_core::EffectValue::Number(1.),
        },
    });
    ready(&w);
    pump(400);
    assert_eq!(inspector.result.borrow().as_ref(), Some(&initial));
    assert!(
        label(&inspector, "histogram-status")
            .text()
            .starts_with("Paused")
    );
    automatic.set_active(true);
    let adjusted = completed(&inspector);
    assert_eq!((adjusted.pixels, adjusted.transparent), (768, 256));
    assert!(adjusted.channels[..3].iter().all(|c| c.above == 256));
    let after = super::place_source::snapshot(&w);
    // Changes while a capture may be active coalesce to the final revision.
    for value in [0.3, 0.7, -1.] {
        w.dispatch(UiAction::Effect {
            action: EffectAction::Set {
                layer: effect.0,
                key: "exposure".into(),
                value: layer_core::EffectValue::Number(value),
            },
        });
        pump(170);
    }
    ready(&w);
    pump(350);
    let final_result = completed(&inspector);
    assert!(
        final_result.channels[..3]
            .iter()
            .all(|c| c.above == 0 && c.white == 0)
    );
    assert_ne!(final_result, adjusted);
    inspector.window.close();
    pump(200);
    assert!(!inspector.window.is_visible());
    invoke(&w, CommandId::Histogram);
    finish(&w);
    assert!(Rc::ptr_eq(
        w.histogram.borrow().as_ref().unwrap(),
        &inspector
    ));
    assert_eq!(completed(&inspector), final_result);
    assert_ne!(super::place_source::snapshot(&w), after);
    automatic.set_active(false);
    automatic.set_active(true);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !label(&inspector, "histogram-status")
        .text()
        .contains("Updating…")
    {
        pump(1);
        assert!(Instant::now() < deadline);
    }
    // Close during a real worker capture, then reuse the same inspector. Its
    // successor cannot publish the cancelled job or overlap a second worker.
    inspector.window.close();
    invoke(&w, CommandId::Histogram);
    finish(&w);
    assert_eq!(completed(&inspector), final_result);
    inspector.window.close();
    w.window.close();
    pump(100);
}
