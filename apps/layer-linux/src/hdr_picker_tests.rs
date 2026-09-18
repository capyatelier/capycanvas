//! Native HDR picker transport and real input on the isolated Mutter display.
use super::new_photo::{capture_ui, ready};
use super::*;
use gtk::subclass::prelude::*;
use layer_core::color::{RgbColor, RgbSpace, SampleDepth};
use layer_ui::{ColorAction, ColorShape, ColorSlot};

fn field(wheel: &crate::tool_panels::ColorWheel) -> gdk::Texture {
    wheel.imp().disc.borrow().as_ref().unwrap().7.clone()
}
fn peak(texture: &gdk::Texture) -> f32 {
    let mut d = gdk::TextureDownloader::new(texture);
    d.set_color_state(&gdk::ColorState::srgb_linear());
    d.set_format(gdk::MemoryFormat::R32g32b32a32Float);
    let (bytes, stride) = d.download_bytes();
    let mut peak = 0f32;
    for y in 0..texture.height() as usize {
        for x in 0..texture.width() as usize {
            for c in 0..3 {
                let i = y * stride + x * 16 + c * 4;
                peak = peak.max(f32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap()));
            }
        }
    }
    peak
}
#[test]
#[ignore = "Wayland/GPU; optional private native input and LAYER_EXPECT_HDR=1"]
fn native_hdr_picker_intensity_shape_and_input() {
    let app = native_test_app("art.capycanvas.HdrPicker");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("../../artifacts/color-m4/hdr-picker-native"));
    std::fs::create_dir_all(&output).unwrap();
    for hdr in [false, true] {
        let mut p = new_drawing(64, 64).unwrap();
        if hdr {
            if let Some(path) = std::env::var_os("LAYER_HDR_LARGE_INPUT") {
                p = layer_core::Project::read(
                    std::fs::File::open(path).unwrap(),
                    Default::default(),
                )
                .unwrap();
                assert!(u64::from(p.document.width) * u64::from(p.document.height) >= 59_000_000);
            }
            p.document.color.depth = SampleDepth::F16;
        }
        let w = Workspace::with_project(&app, Some((p, None)));
        w.window.maximize();
        w.window.present();
        ready(&w);
        pump(300);
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let mut fixture = layer_ui::WorkspaceState::default();
        fixture
            .layout
            .set_panel_visible(Panel::Color, true)
            .unwrap();
        fixture
            .layout
            .move_panel(
                viewport,
                Panel::Color,
                DockTarget::Float {
                    position: [480., 120.],
                },
            )
            .unwrap();
        for f in &mut fixture.layout.floating {
            if let DockNode::Tabs { panels, .. } = &f.root {
                if panels.contains(&Panel::Color) {
                    f.width = 280.;
                    f.height = Some(400.);
                }
            }
        }
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(fixture.clone()),
        });
        w.dispatch(UiAction::SetColor {
            rgba: [0.3, 0.7, 0.5, 0.4],
        });
        pump(200);
        let root = &w.color_panel.root;
        let control = find_named(root.upcast_ref(), "color-hdr-brightness").unwrap();
        let scale = find_named(root.upcast_ref(), "color-hdr-intensity-ramp")
            .unwrap()
            .downcast::<crate::hdr_color_scale::HdrColorScale>()
            .unwrap();
        let wheel = find_named(root.upcast_ref(), "color-wheel")
            .unwrap()
            .downcast::<crate::tool_panels::ColorWheel>()
            .unwrap();
        assert_eq!(control.is_visible(), hdr);
        if !hdr {
            assert_eq!(
                root.first_child().unwrap(),
                wheel.clone().upcast::<gtk::Widget>()
            );
            super::color_panel::hue_guide(&w);
            capture_ui(&w, &output, "sdr-unchanged.png");
            w.window.destroy();
            pump(100);
            continue;
        }
        assert_eq!(root.first_child().unwrap(), control);
        assert!(scale.compute_bounds(root).unwrap().y() < wheel.compute_bounds(root).unwrap().y());
        assert!(
            scale.range_rect().height() >= 20,
            "thick track: {:?}",
            scale.range_rect()
        );
        capture_ui(&w, &output, "hdr-picker-start.png");
        let (start, end) = scale.slider_range();
        assert!(end - start >= 20, "native circular thumb has area");
        assert!(
            scale.imp().texture.borrow().is_some(),
            "the custom ramp was painted"
        );
        let physical = std::env::var_os("LAYER_EXPECT_HDR").is_some();
        if physical {
            assert!(w.picker_headroom() > 1.);
        }
        for shape in [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle] {
            w.dispatch(UiAction::Color {
                action: ColorAction::Shape { shape },
            });
            pump(100);
            scale.set_value(0.);
            pump(100);
            let ramp_pixels = || {
                let s = gtk::Snapshot::new();
                gtk::WidgetPaintable::new(Some(&scale)).snapshot(
                    &s,
                    scale.width() as f64,
                    scale.height() as f64,
                );
                let t = w
                    .window
                    .renderer()
                    .unwrap()
                    .render_texture(&s.to_node().unwrap(), None);
                let mut pixels = vec![0; t.width() as usize * t.height() as usize * 4];
                t.download(&mut pixels, t.width() as usize * 4);
                pixels
            };
            let ramp_before = ramp_pixels();
            let marker = state(&w).colors.wheel_components();
            let before = field(&wheel);
            let guide = super::color_panel::hue_guide(&w);
            let base = state(&w)
                .colors
                .definition()
                .linear_in(RgbSpace::Srgb)
                .unwrap();
            scale.set_value(2.);
            pump(150);
            let after = field(&wheel);
            assert_ne!(
                ramp_pixels(),
                ramp_before,
                "moving EV must redraw the circular thumb"
            );
            assert_eq!(state(&w).colors.hdr_intensity(), 2.);
            assert_eq!(state(&w).colors.wheel_components(), marker);
            assert_eq!(
                super::color_panel::hue_guide(&w),
                guide,
                "EV keeps hue guide cached"
            );
            assert_ne!(before, after);
            let actual = state(&w)
                .colors
                .definition()
                .linear_in(RgbSpace::Srgb)
                .unwrap();
            for c in 0..3 {
                assert!((actual[c] - base[c] * 4.).abs() < 2e-5);
            }
            assert_eq!(actual[3], base[3]);
            if physical {
                assert_eq!(after.color_state(), gdk::ColorState::rec2100_linear());
                assert!(peak(&after) > 3.5, "float field peak {}", peak(&after));
                let s = gtk::Snapshot::new();
                gtk::WidgetPaintable::new(Some(&scale)).snapshot(
                    &s,
                    scale.width() as f64,
                    scale.height() as f64,
                );
                let t = w
                    .window
                    .renderer()
                    .unwrap()
                    .render_texture(&s.to_node().unwrap(), None);
                assert!(peak(&t) > 1., "GSK must preserve the HDR ramp");
                eprintln!(
                    "HDR_PICKER {shape:?} field_peak={} ramp_gsk_peak={}",
                    peak(&after),
                    peak(&t)
                );
            } else {
                assert!(peak(&after) <= 1.001);
            }
            capture_ui(&w, &output, &format!("hdr-{shape:?}-plus2.png"));
        }
        w.dispatch(UiAction::Color {
            action: ColorAction::Definition {
                color: RgbColor::BLACK,
            },
        });
        scale.set_value(2.);
        pump(60);
        assert!(scale.is_sensitive());
        assert_eq!(
            state(&w)
                .colors
                .definition()
                .linear_in(RgbSpace::Srgb)
                .unwrap(),
            [0., 0., 0., 1.]
        );
        w.dispatch(UiAction::Color {
            action: ColorAction::Select {
                slot: ColorSlot::Transparent,
            },
        });
        assert!(!scale.is_sensitive());
        w.dispatch(UiAction::Color {
            action: ColorAction::Select {
                slot: ColorSlot::Foreground,
            },
        });
        w.dispatch(UiAction::SetColor {
            rgba: [0.3, 0.7, 0.5, 1.],
        });
        w.dispatch(UiAction::Color {
            action: ColorAction::Shape {
                shape: ColorShape::Circle,
            },
        });
        pump(100);
        if let Some(dir) = std::env::var_os("LAYER_NATIVE_INPUT_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::write(dir.join("ready"), "ready").unwrap();
            let mut step = 0;
            let mut perform = |events: serde_json::Value| {
                std::fs::write(
                    dir.join(format!("step-{step}.json.tmp")),
                    serde_json::to_vec(&events).unwrap(),
                )
                .unwrap();
                std::fs::rename(
                    dir.join(format!("step-{step}.json.tmp")),
                    dir.join(format!("step-{step}.json")),
                )
                .unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while !dir.join(format!("done-{step}")).exists() {
                    assert!(Instant::now() < deadline);
                    pump(2);
                }
                step += 1;
                pump(150);
            };
            let locate = |value: f32| {
                let r = scale.range_rect();
                let (a, z) = scale.slider_range();
                let radius = (z - a) as f32 * 0.5;
                let p = scale
                    .compute_point(
                        &w.window,
                        &gtk::graphene::Point::new(
                            r.x() as f32
                                + radius
                                + (r.width() as f32 - 2. * radius) * (value + 2.) / 8.,
                            r.y() as f32 + r.height() as f32 * 0.5,
                        ),
                    )
                    .unwrap();
                [p.x(), p.y()]
            };
            perform(
                serde_json::json!([{"point":locate(0.)},{"down":true},{"point":locate(2.)},{"down":false}]),
            );
            assert!(
                (state(&w).colors.hdr_intensity() - 2.).abs() < 0.08,
                "mouse drag: {}",
                state(&w).colors.hdr_intensity()
            );
            perform(
                serde_json::json!([{"touch":"down","point":locate(2.)},{"touch":"move","point":locate(1.)},{"touch":"up"}]),
            );
            assert!(
                (state(&w).colors.hdr_intensity() - 1.).abs() < 0.12,
                "touch drag: {}",
                state(&w).colors.hdr_intensity()
            );
            scale.grab_focus();
            let before = scale.value();
            perform(serde_json::json!([{"key":65363,"down":true},{"key":65363,"down":false}]));
            assert!(
                (scale.value() - before - 0.1).abs() < 0.02,
                "keyboard arrow"
            );
            // Exercise ordinary SDR picking through the same real pointer,
            // independently of the older hue-gradient comparison tolerance.
            let sdr = Workspace::with_project(&app, Some((new_drawing(64, 64).unwrap(), None)));
            sdr.window.maximize();
            sdr.window.present();
            ready(&sdr);
            pump(200);
            sdr.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(fixture.clone()),
            });
            pump(120);
            let sr = &sdr.color_panel.root;
            assert!(
                !find_named(sr.upcast_ref(), "color-hdr-brightness")
                    .unwrap()
                    .is_visible()
            );
            let sw = find_named(sr.upcast_ref(), "color-wheel")
                .unwrap()
                .downcast::<crate::tool_panels::ColorWheel>()
                .unwrap();
            for shape in [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle] {
                sdr.dispatch(UiAction::SetColor {
                    rgba: [0.1, 0.2, 0.3, 1.],
                });
                sdr.dispatch(UiAction::Color {
                    action: ColorAction::Shape { shape },
                });
                pump(80);
                let before = state(&sdr).colors.definition();
                let (size, origin) = sw.drawing_bounds();
                let g = layer_ui::ColorWheelGeometry::new(size).unwrap();
                let p = sw
                    .compute_point(
                        &sdr.window,
                        &gtk::graphene::Point::new(
                            origin[0] + g.center[0],
                            origin[1] + g.center[1],
                        ),
                    )
                    .unwrap();
                perform(serde_json::json!([{"point":[p.x(),p.y()]},{"down":true},{"down":false}]));
                let colors = state(&sdr).colors;
                assert_ne!(
                    colors.definition(),
                    before,
                    "SDR {shape:?} field remains operable"
                );
                assert_eq!(colors.hdr_intensity(), 0.);
                assert!(colors.rgba().iter().all(|v| (0. ..=1.).contains(v)));
            }
            capture_ui(&sdr, &output, "sdr-native-input.png");
            sdr.window.destroy();
            w.window.present();
            pump(100);
            std::fs::write(dir.join("finished"), "done").unwrap();
        }
        // Exercise repeated updates without document-size-dependent work.
        let before = Instant::now();
        for i in 0..60 {
            scale.set_value((i % 41) as f64 * 0.1);
            pump(1);
        }
        eprintln!(
            "HDR_PICKER_60_UPDATES_MS {:.2}",
            before.elapsed().as_secs_f64() * 1000.
        );
        scale.set_value(2.);
        pump(100);
        capture_ui(&w, &output, "hdr-picker-final.png");
        if physical {
            let original = state(&w).colors.definition();
            let headroom = w.picker_headroom();
            for h in [1., headroom] {
                w.gpu
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .session
                    .renderer_mut()
                    .display_headroom = h;
                w.changed(Ok(layer_ui::UiChange {
                    regions: layer_ui::regions::SETTINGS,
                    ..Default::default()
                }));
                pump(120);
                assert_eq!(
                    field(&wheel).color_state() == gdk::ColorState::rec2100_linear(),
                    h > 1.
                );
                assert_eq!(state(&w).colors.definition(), original);
            }
        }
        for theme in [Theme::Dark, Theme::Light] {
            for f in &mut fixture.layout.floating {
                if let DockNode::Tabs { panels, .. } = &f.root {
                    if panels.contains(&Panel::Color) {
                        f.width = 160.;
                        f.height = Some(320.);
                    }
                }
            }
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(fixture.clone()),
            });
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            w.dispatch(UiAction::SetColor {
                rgba: [0.3, 0.7, 0.5, 1.],
            });
            w.dispatch(UiAction::Color {
                action: ColorAction::HdrIntensity { stops: 2. },
            });
            pump(120);
            assert!(
                root.width() <= 144,
                "HDR controls fit 160px panel: {}",
                root.width()
            );
            for name in [
                "color-hdr-brightness",
                "color-hdr-intensity-ramp",
                "color-edit-button",
                "color-wheel",
            ] {
                let widget = find_named(root.upcast_ref(), name).unwrap();
                let b = widget.compute_bounds(root).unwrap();
                assert!(
                    b.x() >= 0. && b.x() + b.width() <= root.width() as f32 + 0.5,
                    "{name} fits narrow panel: {b:?}"
                );
            }
            capture_ui(&w, &output, &format!("hdr-narrow-{theme:?}.png"));
        }
        w.window.destroy();
        pump(100);
    }
}
