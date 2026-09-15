//! SDR workflow checks through native controls and the production render worker.
use super::*;

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_sdr_sampling_controls() {
    let app = native_test_app("art.capycanvas.SdrSampling");
    let w = Workspace::with_project(&app, Some((new_drawing(128, 128).unwrap(), None)));
    w.window.present();
    let ready = |w: &Rc<Workspace>| {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            pump(20);
            if w.gpu.borrow().as_ref().is_some_and(|g| {
                g.session.engine().backend().startup.complete
                    && !g.session.state().filter_load.pending
                    && !g.session.engine().has_pending_document_edits()
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "canvas startup");
        }
    };
    ready(&w);
    let image = layer_core::ProjectAsset {
        extent: [128, 128],
        format: layer_core::ProjectAssetFormat::Rgba8Srgb,
        bytes: (0..128 * 128)
            .flat_map(|i| {
                if (i / 128 + i % 128) % 2 == 0 {
                    [0, 0, 255, 0]
                } else {
                    [255, 0, 0, 255]
                }
            })
            .collect(),
    };
    w.gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .import_layer_asset("Sampling", image)
        .unwrap();
    w.refresh(regions::DOCUMENT | regions::COMMANDS);
    w.wake();
    ready(&w);
    w.dispatch(UiAction::Layer {
        action: LayerAction::Tool {
            tool: LayerCanvasTool::PickLayer,
        },
    });
    w.dispatch(UiAction::SetBrushOpacity { value: 0.37 });
    let revision = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .revision;
    for width in [1, 3, 5] {
        let button = w
            .tool_set
            .buttons
            .borrow()
            .iter()
            .find(|(item, _, _)| item.action == UiAction::SetColorSampleSize { width })
            .unwrap()
            .1
            .clone();
        button.emit_clicked();
        pump(20);
        assert!(
            state(&w)
                .tool_set
                .subtools
                .iter()
                .any(|item| item.selected && item.action == UiAction::SetColorSampleSize { width })
        );
        w.dispatch(UiAction::SetColor {
            rgba: [0., 0., 1., 1.],
        });
        let camera = state(&w).camera;
        let m = camera.document_to_surface();
        for (sequence, phase) in [(1, PenPhase::Down), (2, PenPhase::Up)] {
            w.gpu
                .borrow_mut()
                .as_mut()
                .unwrap()
                .session
                .pen(PenEvent {
                    device_id: 91,
                    sequence,
                    timestamp_ns: glib::monotonic_time() as u64 * 1000,
                    view_revision: camera.revision,
                    surface_position: Point {
                        x: m[0] * 64.5 + m[2] * 64.5 + m[4],
                        y: m[1] * 64.5 + m[3] * 64.5 + m[5],
                    },
                    pressure: 1.,
                    tilt_radians: [0.; 2],
                    twist_radians: 0.,
                    distance: 0.,
                    phase,
                    tool: ToolKind::Pen,
                    flags: SampleFlags::PRIMARY,
                })
                .unwrap();
        }
        w.wake();
        pump(200);
        let expected = if width == 1 {
            [0., 0., 1., 1.]
        } else {
            [1., 0., 0., 1.]
        };
        for (actual, expected) in state(&w).colors.rgba().into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.000001);
        }
        assert_eq!(state(&w).brush.opacity, 0.37);
        assert_eq!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .document()
                .revision,
            revision
        );
    }
    w.window.destroy();
    pump(100);
}

fn sdr_ready(w: &Rc<Workspace>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        pump(20);
        if w.gpu.borrow().as_ref().is_some_and(|g| {
            g.session.engine().backend().startup.complete
                && !g.session.state().filter_load.pending
                && !g.session.engine().has_pending_document_edits()
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "SDR canvas startup: {}",
            w.status.text()
        );
    }
}

fn pixels(w: &Rc<Workspace>, id: u32) -> Vec<u8> {
    glib::MainContext::default()
        .block_on(read_canvas_pixels(w, id))
        .unwrap()
        .bytes
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_numeric_colors_and_saved_palettes() {
    use layer_core::color::{DocumentColor, IntegerDepth, RgbColor, RgbSpace};
    use layer_ui::{ColorAction, ColorInputModel, ColorSlot};
    let app = native_test_app("art.capycanvas.NumericColors");
    let output = std::path::Path::new("../../artifacts/color-m2/numeric-palette-ui");
    std::fs::create_dir_all(output).unwrap();
    let mut project = new_drawing(256, 256).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    };
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    sdr_ready(&w);
    let original = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0.1234567, 123. / 65535.]).unwrap();
    w.dispatch(UiAction::Color {
        action: ColorAction::Definition { color: original },
    });
    w.dispatch(UiAction::SetBrushOpacity { value: 0.37 });
    let revision = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .revision;
    let click_named = |w: &Rc<Workspace>, name: &str| {
        let button = find_named(w.window.upcast_ref(), name)
            .unwrap_or_else(|| panic!("{name}"))
            .downcast::<gtk::Button>()
            .unwrap();
        button.emit_clicked();
        pump(100);
    };
    let respond = |w: &Rc<Workspace>, label: &str| {
        let dialog = w.window.visible_dialog().unwrap();
        click(&find_button(dialog.upcast_ref(), label).unwrap());
        pump(100);
    };
    click_named(&w, "color-edit-menu");
    capture_reference(
        &w,
        output.join("numeric-document-rgb.png").to_str().unwrap(),
        1.,
    );
    let dialog = w.window.visible_dialog().unwrap();
    let model = find_named(dialog.upcast_ref(), "edit-color-model")
        .unwrap()
        .downcast::<adw::ComboRow>()
        .unwrap();
    for selected in 0..ColorInputModel::ALL.len() {
        model.set_selected(selected as u32);
        pump(10);
    }
    capture_reference(&w, output.join("numeric-oklch.png").to_str().unwrap(), 1.);
    respond(&w, "Use Color");
    assert_eq!(
        state(&w).colors.definition(),
        original,
        "unchanged models never quantize RGB/alpha"
    );
    click_named(&w, "color-edit-menu");
    let dialog = w.window.visible_dialog().unwrap();
    let entry = find_named(dialog.upcast_ref(), "edit-color-value-0")
        .unwrap()
        .downcast::<adw::EntryRow>()
        .unwrap();
    entry.set_text("NaN");
    pump(10);
    assert!(
        !find_button(dialog.upcast_ref(), "Use Color")
            .unwrap()
            .is_sensitive()
    );
    respond(&w, "Cancel");
    assert_eq!(state(&w).colors.definition(), original);
    click_named(&w, "color-edit-menu");
    let dialog = w.window.visible_dialog().unwrap();
    let model = find_named(dialog.upcast_ref(), "edit-color-model")
        .unwrap()
        .downcast::<adw::ComboRow>()
        .unwrap();
    model.set_selected(1);
    pump(10);
    let entry = find_named(dialog.upcast_ref(), "edit-color-value-0")
        .unwrap()
        .downcast::<adw::EntryRow>()
        .unwrap();
    entry.set_text("#12A5E3");
    respond(&w, "Use Color");
    let hex = state(&w).colors.definition();
    assert_eq!(hex.space, RgbSpace::Srgb);
    assert_eq!(
        hex.rgba,
        [18. / 255., 165. / 255., 227. / 255., original.rgba[3]]
    );
    assert_eq!(state(&w).brush.opacity, 0.37);
    w.dispatch(UiAction::Color {
        action: ColorAction::Definition { color: original },
    });
    click_named(&w, "color-library-menu");
    click_named(&w, "color-library-create");
    let dialog = w.window.visible_dialog().unwrap();
    find_named(dialog.upcast_ref(), "color-library-name")
        .unwrap()
        .downcast::<adw::EntryRow>()
        .unwrap()
        .set_text("Photo colors");
    respond(&w, "Save");
    click_named(&w, "color-library-store");
    let dialog = w.window.visible_dialog().unwrap();
    find_named(dialog.upcast_ref(), "color-library-name")
        .unwrap()
        .downcast::<adw::EntryRow>()
        .unwrap()
        .set_text("Wide red");
    respond(&w, "Save");
    let palette = state(&w).colors.library.palettes.last().unwrap().clone();
    assert_eq!(palette.name, "Photo colors");
    assert_eq!(palette.swatches.len(), 1);
    assert_eq!(palette.swatches[0].color, original);
    capture_reference(
        &w,
        output.join("saved-wide-swatch.png").to_str().unwrap(),
        1.,
    );
    click_named(&w, "color-library-create");
    let dialog = w.window.visible_dialog().unwrap();
    find_named(dialog.upcast_ref(), "color-library-name")
        .unwrap()
        .downcast::<adw::EntryRow>()
        .unwrap()
        .set_text("PHOTO COLORS");
    pump(10);
    assert!(
        !find_button(dialog.upcast_ref(), "Save")
            .unwrap()
            .is_sensitive()
    );
    respond(&w, "Cancel");
    assert_eq!(state(&w).colors.library.palettes.len(), 2);
    click_named(
        &w,
        &format!("saved-color-{}-Rename", palette.swatches[0].id),
    );
    let dialog = w.window.visible_dialog().unwrap();
    find_named(dialog.upcast_ref(), "color-library-name")
        .unwrap()
        .downcast::<adw::EntryRow>()
        .unwrap()
        .set_text("P3 low-alpha red");
    respond(&w, "Save");
    assert_eq!(
        state(&w)
            .colors
            .library
            .swatch(palette.swatches[0].id)
            .unwrap()
            .name,
        "P3 low-alpha red"
    );
    respond(&w, "Close");
    w.dispatch(UiAction::Color {
        action: ColorAction::Definition {
            color: RgbColor::WHITE,
        },
    });
    click_named(&w, "color-library-menu");
    let dialog = w.window.visible_dialog().unwrap();
    find_named(dialog.upcast_ref(), "color-library-palette")
        .unwrap()
        .downcast::<adw::ComboRow>()
        .unwrap()
        .set_selected(1);
    pump(50);
    let row = find_named(
        dialog.upcast_ref(),
        &format!("saved-color-{}", palette.swatches[0].id),
    )
    .unwrap()
    .downcast::<adw::ActionRow>()
    .unwrap();
    adw::prelude::ActionRowExt::activate(&row);
    pump(100);
    assert_eq!(state(&w).colors.definition(), original);
    assert!(w.window.visible_dialog().is_none());
    // Workspace serialization carries the actual wide definition. Restore it in
    // another native document and use the swatch in the other paint slot.
    let capture = w
        .gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .capture_workspace()
        .unwrap();
    let bytes = serde_json::to_vec(&capture).unwrap();
    let next = Workspace::with_project(&app, Some((new_drawing(128, 128).unwrap(), None)));
    next.window.present();
    sdr_ready(&next);
    let prepared =
        layer_ui::PreparedWorkspace::new(serde_json::from_slice(&bytes).unwrap()).unwrap();
    let change = next
        .gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .adopt_workspace(prepared);
    next.changed(change);
    crate::color_library::show(&next, ColorSlot::Background);
    pump(100);
    let dialog = next.window.visible_dialog().unwrap();
    find_named(dialog.upcast_ref(), "color-library-palette")
        .unwrap()
        .downcast::<adw::ComboRow>()
        .unwrap()
        .set_selected(1);
    pump(50);
    adw::prelude::ActionRowExt::activate(
        &find_named(
            dialog.upcast_ref(),
            &format!("saved-color-{}", palette.swatches[0].id),
        )
        .unwrap()
        .downcast::<adw::ActionRow>()
        .unwrap(),
    );
    pump(100);
    assert_eq!(state(&next).colors.background, original);
    assert_eq!(state(&next).colors.rgb_space(), RgbSpace::Srgb);
    assert_eq!(
        next.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .configured_brush()
            .color_rgba_linear,
        original.linear_in(RgbSpace::Srgb).unwrap()
    );
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .revision,
        revision
    );
    crate::color_library::show(&next, ColorSlot::Background);
    pump(100);
    let dialog = next.window.visible_dialog().unwrap();
    find_named(dialog.upcast_ref(), "color-library-palette")
        .unwrap()
        .downcast::<adw::ComboRow>()
        .unwrap()
        .set_selected(1);
    pump(50);
    click_named(
        &next,
        &format!("saved-color-{}-Remove", palette.swatches[0].id),
    );
    assert!(
        state(&next)
            .colors
            .library
            .swatch(palette.swatches[0].id)
            .is_none()
    );
    let dialog = next.window.visible_dialog().unwrap();
    click(&find_button(dialog.upcast_ref(), "Remove Palette…").unwrap());
    pump(100);
    respond(&next, "Cancel");
    assert_eq!(state(&next).colors.library.palettes.len(), 2);
    let dialog = next.window.visible_dialog().unwrap();
    click(&find_button(dialog.upcast_ref(), "Remove Palette…").unwrap());
    pump(100);
    respond(&next, "Remove");
    assert_eq!(state(&next).colors.library.palettes.len(), 1);
    assert_eq!(state(&next).colors.background, original);
    respond(&next, "Close");
    next.window.destroy();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_sdr_portable_paint_and_sampling() {
    use layer_core::color::{DocumentColor, IntegerDepth, RgbColor, RgbSpace};
    let app = native_test_app("art.capycanvas.PortablePaint");
    let definition = RgbColor::new(RgbSpace::DisplayP3, [0.68, 0.23, 0.47, 1.]).unwrap();
    for space in RgbSpace::ALL {
        let mut project = new_drawing(256, 256).unwrap();
        project.document.color = DocumentColor {
            space,
            depth: IntegerDepth::U16,
        };
        let w = Workspace::with_project(&app, Some((project, None)));
        w.window.present();
        super::new_photo::ready(&w);
        assert_eq!(state(&w).colors.rgb_space(), space);
        // The next window can restore the previous window's eyedropper tool.
        // Select painting explicitly before creating the pixels to sample.
        w.dispatch(UiAction::Layer {
            action: LayerAction::Tool { tool: LayerCanvasTool::Paint },
        });
        w.dispatch(UiAction::Color {
            action: layer_ui::ColorAction::Definition { color: definition },
        });
        w.dispatch(UiAction::SetBrushSize { value: 80. });
        assert_eq!(state(&w).colors.definition(), definition);
        assert_eq!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .configured_brush()
                .color_rgba_linear,
            definition.linear_in(space).unwrap()
        );
        native_pen_path(
            &w,
            &[
                [40., 128.],
                [80., 128.],
                [128., 128.],
                [170., 128.],
                [210., 128.],
            ],
        );
        sdr_ready(&w);
        let before = pixels(&w, 9810);
        // Eyedropper goes through the host contact route and asynchronous exact
        // sample result. Every averaging size samples the fully covered center.
        for width in [1, 3, 5] {
            w.dispatch(UiAction::Layer {
                action: LayerAction::Tool {
                    tool: LayerCanvasTool::PickLayer,
                },
            });
            w.dispatch(UiAction::SetColorSampleSize { width });
            w.dispatch(UiAction::SetBrushOpacity { value: 0.37 });
            w.dispatch(UiAction::Color {
                action: layer_ui::ColorAction::Definition {
                    color: RgbColor::WHITE,
                },
            });
            native_pen_path(&w, &[[128., 128.], [128., 128.]]);
            let deadline = Instant::now() + Duration::from_secs(10);
            while state(&w).colors.definition() == RgbColor::WHITE {
                pump(10);
                assert!(Instant::now() < deadline, "{space:?} {width}: exact sample did not complete");
            }
            let picked = state(&w).colors.definition();
            assert_eq!(picked.space, space);
            let expected = definition.encoded_in(space).unwrap();
            for (a, b) in picked.rgba.into_iter().zip(expected) {
                // Native U16 backing rounds once in encoded document RGB.
                assert!(
                    (a - b).abs() < 2. / 65535.,
                    "{space:?} {width}: {picked:?} != {expected:?}"
                );
            }
            assert_eq!(state(&w).brush.opacity, 0.37);
            assert_eq!(pixels(&w, 9811), before, "sampling changes no pixels");
        }
        for shape in [
            layer_ui::ColorShape::Circle,
            layer_ui::ColorShape::Square,
            layer_ui::ColorShape::Triangle,
        ] {
            let before = state(&w).colors.definition();
            w.dispatch(UiAction::Color {
                action: layer_ui::ColorAction::Shape { shape },
            });
            pump(50);
            assert_eq!(state(&w).colors.definition(), before);
        }
        w.window.destroy();
        pump(50);
    }
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_sdr_document_modes() {
    use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace, source::*};
    use layer_render::CanvasRenderer;
    use std::sync::Arc;
    let app = native_test_app("art.capycanvas.SdrDocuments");
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let color = DocumentColor { space, depth };
            let mut project = new_drawing(513, 257).unwrap();
            project.document.color = color;
            let mut builder = SourceBuilder::new(
                [513, 257],
                SourceInterpretation {
                    channels: SourceChannels::Rgba,
                    depth,
                    profile: ColorProfile::Icc(
                        layer_color::profile_bytes(&ColorProfile::Builtin(space))
                            .unwrap()
                            .into(),
                    ),
                    profile_assumed: false,
                },
                16 * 1024 * 1024,
            )
            .unwrap();
            for y in 0..257 {
                let mut row = Vec::new();
                for x in 0..513 {
                    for value in [((x * 71 + y * 37) % 65536) as u16, 31001, 52999, 50000] {
                        match depth {
                            IntegerDepth::U8 => row.push((value / 257) as u8),
                            IntegerDepth::U16 => row.extend_from_slice(&value.to_le_bytes()),
                        }
                    }
                }
                builder.push_row(&row).unwrap();
            }
            let source = Arc::new(builder.finish().unwrap());
            project.document.layers[0].source = Some(source.clone());
            let w = Workspace::with_project(&app, Some((project, None)));
            w.window.present();
            sdr_ready(&w);
            assert_eq!(
                w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .engine()
                    .backend()
                    .document_color(),
                color
            );
            let original = pixels(&w, 9100);
            w.dispatch(UiAction::SetColor {
                rgba: [0., 0., 0., 1.],
            });
            w.dispatch(UiAction::SetBrushSize { value: 21. });
            sdr_ready(&w);
            native_pen_path(
                &w,
                &[[230., 120.], [250., 120.], [270., 120.], [285., 120.]],
            );
            sdr_ready(&w);
            let painted = pixels(&w, 9101);
            assert_ne!(
                painted, original,
                "{color:?} brush draws over retained photo"
            );
            let root = w
                .gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .engine()
                .document()
                .layers[0]
                .raster
                .clone();
            let backing = root.wait_data().unwrap();
            assert!(backing.tiles.len() >= 2, "stroke crosses page boundary");
            for (key, tile) in &backing.tiles {
                assert_eq!(
                    tile.wait_backing().unwrap().descriptor,
                    key.plane.descriptor(color)
                );
            }
            w.dispatch(UiAction::Invoke {
                command: CommandId::Undo,
            });
            sdr_ready(&w);
            assert_eq!(pixels(&w, 9102), original, "{color:?} undo");
            w.dispatch(UiAction::Invoke {
                command: CommandId::Redo,
            });
            sdr_ready(&w);
            assert_eq!(pixels(&w, 9103), painted, "{color:?} redo");
            let project = w
                .gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .capture_project_recovery()
                .unwrap();
            assert!(Arc::ptr_eq(
                project.document.layers[0].source.as_ref().unwrap(),
                &source
            ));
            let mut bytes = Vec::new();
            project.write(&mut bytes).unwrap();
            let reopened =
                layer_core::Project::read(std::io::Cursor::new(bytes), Default::default()).unwrap();
            assert_eq!(reopened.document.color, color);
            assert_eq!(
                reopened.document.layers[0].source.as_ref().unwrap(),
                &source
            );
            let restored = reopened.document.layers[0].raster.wait_data().unwrap();
            for (key, tile) in &backing.tiles {
                assert_eq!(
                    tile.wait_backing().unwrap().decode().unwrap(),
                    restored.tiles[key]
                        .wait_backing()
                        .unwrap()
                        .decode()
                        .unwrap()
                );
            }
            w.window.destroy();
            pump(50);
            let w = Workspace::with_project(&app, Some((reopened, None)));
            w.window.present();
            sdr_ready(&w);
            assert_eq!(
                pixels(&w, 9104),
                painted,
                "{color:?} reopened native window"
            );
            w.window.destroy();
            pump(50);
        }
    }
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_sdr_bounded_canvas_startup_and_paint() {
    use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};
    use layer_render::{CanvasRenderer, ColorSampleArea, ColorSampleRequest, ColorSampleSource};
    let app = native_test_app("art.capycanvas.SdrBoundedCanvas");
    // Exceeds the dense Float32 display ceiling and starts zoomed out in a real
    // GTK window, including the host's initial paper presentation and warmup.
    let mut project = new_drawing(4097, 1025).unwrap();
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    };
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    sdr_ready(&w);
    w.dispatch(UiAction::SetColor {
        rgba: [0., 0., 0., 1.],
    });
    w.dispatch(UiAction::SetBrushSize { value: 80. });
    sdr_ready(&w);
    native_pen_path(
        &w,
        &[[1990., 512.], [2030., 512.], [2070., 512.], [2100., 512.]],
    );
    sdr_ready(&w);
    assert!(
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .renderer_mut()
            .request_color_sample(ColorSampleRequest {
                request_id: 9700,
                source: ColorSampleSource::Composite,
                position: [2048, 512],
                area: ColorSampleArea::Point,
            })
            .unwrap()
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let sample = loop {
        pump(10);
        let sample = w
            .gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .renderer_mut()
            .take_color_sample();
        if let Some(sample) = sample {
            break sample.unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "exact sample after bounded display"
        );
    };
    assert_eq!(sample.request_id, 9700);
    assert!(
        sample.rgba[..3].iter().all(|v| v.abs() < 0.0001),
        "{:?}",
        sample.rgba
    );
    assert_eq!(sample.rgba[3], 1.);
    w.window.destroy();
    pump(50);
}
