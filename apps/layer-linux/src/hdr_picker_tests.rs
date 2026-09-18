//! Native HDR picker transport and real input on the isolated Mutter display.
use super::new_photo::{capture_ui, ready, response};
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
fn patch_texture(widget: &gtk::Widget) -> gdk::Texture {
    widget.downcast_ref::<crate::display_color::ColorPatch>().unwrap().imp().textures.borrow().as_ref().unwrap()[0].clone()
}
fn paint_patch(w: &Workspace, slot: ColorSlot) -> gtk::Widget {
    find_named(w.color_panel.root.upcast_ref(), &format!("color-{slot:?}")).unwrap()
        .downcast::<gtk::Button>().unwrap().child().unwrap()
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
        let scale = find_named(root.upcast_ref(), "color-hdr-intensity-ramp")
            .unwrap()
            .downcast::<crate::hdr_color_scale::HdrColorScale>()
            .unwrap();
        let wheel = find_named(root.upcast_ref(), "color-wheel")
            .unwrap()
            .downcast::<crate::tool_panels::ColorWheel>()
            .unwrap();
        assert_eq!(scale.is_visible(), hdr);
        let pencil = find_named(root.upcast_ref(), "color-edit-button").unwrap().downcast::<gtk::Button>().unwrap();
        let swap = find_named(root.upcast_ref(), "color-swap").unwrap();
        assert_eq!([pencil.width(), pencil.height()], [swap.width(), swap.height()]);
        let pencil_bounds = pencil.compute_bounds(root).unwrap();
        let label_bounds = find_named(root.upcast_ref(), "color-readout").unwrap().compute_bounds(root).unwrap();
        assert_eq!(pencil_bounds.y(), label_bounds.y());
        assert!(pencil_bounds.x() > label_bounds.x() + label_bounds.width());
        pencil.emit_clicked();
        pump(100);
        let entry = find_named(w.window.visible_dialog().unwrap().upcast_ref(), "edit-color-ev").unwrap();
        assert_eq!(entry.is_visible(), hdr);
        response(&w, "cancel");
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
        assert_eq!(root.first_child().unwrap(), wheel.clone().upcast::<gtk::Widget>());
        let arc = scale.geometry().unwrap();
        assert!((arc.width - wheel.drawing_bounds().0 * 0.11).abs() < 0.01);
        assert!(!scale.contains(arc.center[0] as f64, arc.center[1] as f64));
        assert!(find_named(root.upcast_ref(), "color-hdr-brightness").is_none());
        capture_ui(&w, &output, "hdr-picker-start.png");
        assert!(
            scale.imp().texture.borrow().is_some(),
            "the custom ramp was painted"
        );
        scale.set_value(2.);
        let original = state(&w).colors;
        pencil.emit_clicked();
        pump(100);
        let dialog = w.window.visible_dialog().unwrap();
        let base_preview = find_named(dialog.upcast_ref(), "edit-color-base-preview").unwrap();
        let adjusted_preview = find_named(dialog.upcast_ref(), "edit-color-preview").unwrap();
        let base_bounds = base_preview.compute_bounds(&dialog).unwrap();
        let adjusted_bounds = adjusted_preview.compute_bounds(&dialog).unwrap();
        assert!((base_bounds.width() - adjusted_bounds.width()).abs() <= 1., "Equal halves within native pixel rounding");
        assert!((base_bounds.x() + base_bounds.width() - adjusted_bounds.x()).abs() < 0.01);
        let base_peak = peak(&patch_texture(&base_preview));
        let adjusted_peak = peak(&patch_texture(&adjusted_preview));
        assert!(adjusted_peak > base_peak + 0.05, "EV comparison must show the different renditions");
        assert!((peak(&patch_texture(&paint_patch(&w, ColorSlot::Foreground))) - adjusted_peak).abs() < 0.002, "Paint bubble matches adjusted dialog preview");
        // Numerical alpha check above the display shoulder: alpha changes the
        // checker blend, never the straight color's HDR mapping.
        for alpha in [0., 0.25, 0.5, 1.] {
            let color = RgbColor::from_linear(RgbSpace::Srgb, [8., 8., 8., alpha]).unwrap();
            let textures = crate::display_color::checker_textures(color, w.view_color(), 4.);
            for (texture, checker) in textures.iter().zip([0.94, 0.80]) {
                let expected = (4. - 1. / 6.) * alpha + checker * (1. - alpha);
                assert!((peak(texture) - expected).abs() < 0.005, "HDR alpha {alpha}: {} != {expected}", peak(texture));
            }
        }
        let entry = find_named(dialog.upcast_ref(), "edit-color-ev").unwrap().downcast::<adw::EntryRow>().unwrap();
        assert_eq!(entry.text().parse::<f32>().unwrap(), 2.);
        entry.set_text("3");
        pump(60);
        assert!((peak(&patch_texture(&base_preview)) - base_peak).abs() < 0.002, "EV keeps base preview stable");
        assert!(peak(&patch_texture(&adjusted_preview)) > adjusted_peak + 0.01);
        if std::env::var_os("LAYER_EXPECT_HDR").is_some() {
            assert!(adjusted_peak > 1., "Adjusted preview retains HDR light");
            assert_eq!(patch_texture(&adjusted_preview).color_state(), gdk::ColorState::rec2100_linear());
            for widget in [&adjusted_preview, &paint_patch(&w, ColorSlot::Foreground)] {
                let snapshot = gtk::Snapshot::new();
                gtk::WidgetPaintable::new(Some(widget)).snapshot(&snapshot, widget.width() as f64, widget.height() as f64);
                let texture = w.window.renderer().unwrap().render_texture(&snapshot.to_node().unwrap(), None);
                assert!(peak(&texture) > 1., "GSK retains the HDR paint preview");
                eprintln!("HDR_COLOR_PREVIEW {} gsk_peak={}", widget.widget_name(), peak(&texture));
            }
            let headroom = w.picker_headroom();
            for h in [1., headroom] {
                w.gpu.borrow_mut().as_mut().unwrap().session.renderer_mut().display_headroom = h;
                w.changed(Ok(layer_ui::UiChange { regions: layer_ui::regions::SETTINGS, ..Default::default() }));
                pump(80);
                assert_eq!(patch_texture(&adjusted_preview).color_state() == gdk::ColorState::rec2100_linear(), h > 1.);
                assert_eq!(patch_texture(&paint_patch(&w, ColorSlot::Foreground)).color_state() == gdk::ColorState::rec2100_linear(), h > 1.);
                assert_eq!(entry.text(), "3");
                assert_eq!(state(&w).colors.definition(), original.definition());
            }
        }
        entry.set_text("2");
        entry.set_text("NaN");
        assert!(!find_button(dialog.upcast_ref(), "Use Color").unwrap().is_sensitive());
        let red = find_named(dialog.upcast_ref(), "edit-color-value-0").unwrap().downcast::<adw::EntryRow>().unwrap();
        red.set_text("1");
        assert!(!find_button(dialog.upcast_ref(), "Use Color").unwrap().is_sensitive());
        entry.set_text("3");
        assert!((red.text().parse::<f32>().unwrap() - 2.).abs() < 1e-5);
        capture_ui(&w, &output, "hdr-edit-ev.png");
        response(&w, "cancel");
        assert_eq!(state(&w).colors, original, "Cancel never changes paint or EV");
        pencil.emit_clicked();
        pump(100);
        response(&w, "apply");
        assert_eq!(state(&w).colors, original, "Untouched Edit Color retains exact state");
        pencil.emit_clicked();
        pump(100);
        find_named(w.window.visible_dialog().unwrap().upcast_ref(), "edit-color-ev").unwrap().downcast::<adw::EntryRow>().unwrap().set_text("3");
        response(&w, "apply");
        assert_eq!(state(&w).colors.hdr_intensity(), 3.);
        scale.set_value(0.);
        let physical = std::env::var_os("LAYER_EXPECT_HDR").is_some();
        w.dispatch(UiAction::Color { action: ColorAction::SetSlotIntensity {
            slot: ColorSlot::Background,
            color: RgbColor::from_linear(RgbSpace::Srgb, [4., 2., 1., 1.]).unwrap(), stops: 2.,
        }});
        w.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Foreground } });
        let background = patch_texture(&paint_patch(&w, ColorSlot::Background));
        if physical { assert!(peak(&background) > 3.5, "Secondary bubble retains adjusted HDR color"); }
        else { assert!(peak(&background) <= 1.001); }
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
                let arc = scale.geometry().unwrap();
                let [x, y] = arc.point((value + 2.) / 8.);
                let p = scale.compute_point(&w.window, &gtk::graphene::Point::new(x, y))
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
            perform(serde_json::json!([{"wait_ms":500},{"point":locate(3.)},{"down":true},{"down":false},{"wait_ms":50},{"down":true},{"down":false}]));
            assert_eq!(state(&w).colors.hdr_intensity(), 0., "Double-click resets to 1×, not +1 EV");
            let arc = scale.geometry().unwrap();
            let radius = arc.radius + arc.width * 0.5 + 10.;
            let angle = 76f32.to_radians();
            let point = scale.compute_point(&w.window, &gtk::graphene::Point::new(arc.center[0] + radius * angle.cos(), arc.center[1] + radius * angle.sin())).unwrap();
            let original = state(&w).colors;
            perform(serde_json::json!([{"wait_ms":500},{"point":[point.x(),point.y()]},{"down":true},{"down":false}]));
            assert!(w.window.visible_dialog().is_none(), "EV caption is read-only");
            assert_eq!(state(&w).colors, original, "EV caption does not pick through to the wheel");
            let point = pencil.compute_point(&w.window, &gtk::graphene::Point::new(pencil.width() as f32 * 0.5, pencil.height() as f32 * 0.5)).unwrap();
            perform(serde_json::json!([{"point":[point.x(),point.y()]},{"down":true},{"down":false}]));
            assert_eq!(w.window.visible_dialog().unwrap().widget_name(), "edit-color-dialog");
            response(&w, "cancel");
            for slot in [ColorSlot::Foreground, ColorSlot::Background] {
                let swatch = find_named(root.upcast_ref(), &format!("color-{slot:?}")).unwrap();
                let point = swatch.compute_point(&w.window, &gtk::graphene::Point::new(swatch.width() as f32 * 0.7, swatch.height() as f32 * 0.75)).unwrap();
                let original = if slot == ColorSlot::Foreground { state(&w).colors.foreground } else { state(&w).colors.background };
                perform(serde_json::json!([{"wait_ms":500},{"point":[point.x(),point.y()]},{"down":true},{"down":false},{"wait_ms":50},{"down":true},{"down":false}]));
                let dialog = w.window.visible_dialog().expect("Double-click opens Edit Color");
                assert_eq!(dialog.widget_name(), "edit-color-dialog");
                assert!(find_named(dialog.upcast_ref(), "edit-color-ev").unwrap().is_visible());
                let red = find_named(dialog.upcast_ref(), "edit-color-value-0").unwrap().downcast::<adw::EntryRow>().unwrap().text().parse::<f32>().unwrap();
                assert!((red - original.linear_in(RgbSpace::Srgb).unwrap()[0]).abs() < 1e-5, "Editor belongs to the double-clicked swatch");
                response(&w, "apply");
                assert_eq!(if slot == ColorSlot::Foreground { state(&w).colors.foreground } else { state(&w).colors.background }, original);
            }
            w.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Foreground } });
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
                !find_named(sr.upcast_ref(), "color-hdr-intensity-ramp")
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
