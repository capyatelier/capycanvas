//! Native control/lifecycle integration on a hardware desktop. Control signals
//! exercise GTK bindings; pen records exercise scheduling and GPU presentation.
//! Physical tablet/touch delivery remains a human test (not faked here).
#[path = "native_penup_tests.rs"]
mod native_penup;
#[path = "prediction_tests.rs"]
mod prediction;
#[path = "stroke_recording_tests.rs"]
mod stroke_recording;
#[path = "native_navigation_tests.rs"]
mod native_navigation;
#[path = "editing_tools_tests.rs"]
mod editing_tools;
#[path = "color_panel_tests.rs"]
mod color_panel;
#[path = "color_management_tests.rs"]
mod color_management;
#[path = "color_editor_tests.rs"]
mod color_editor;
#[path = "proof_tests.rs"]
mod proof;
#[path = "effect_color_tests.rs"]
mod effect_color;
#[path = "export_resize_tests.rs"]
mod export_resize;
#[path = "new_photo_tests.rs"]
pub(crate) mod new_photo;
#[path = "store_capture.rs"]
mod store_capture;
#[path = "hdr_tests.rs"]
mod hdr;
#[path = "hdr_picker_tests.rs"]
mod hdr_picker;
#[path = "place_source_tests.rs"]
mod place_source;
#[path = "photo_drop_tests.rs"]
mod photo_drop;
#[path = "canvas_bar_tests.rs"]
mod canvas_bar_tests;
mod clipboard_tests;
#[path = "notice_tests.rs"]
mod notice;
#[path = "zoom_readout_tests.rs"]
mod zoom_readout;
#[path = "blend_menu_tests.rs"]
mod blend_menu;
#[path = "blending_tests.rs"]
mod blending;
#[path = "photo_edit_tests.rs"]
mod photo_edit;
#[path = "object_foundation_tests.rs"]
mod object_foundation;
#[path = "calibration_tests.rs"]
mod calibration;
#[path = "merge_tests.rs"]
mod merge;
#[path = "crop_tests.rs"]
mod crop;
#[path = "image_tests.rs"]
mod image;
#[path = "clone_tests.rs"]
mod clone_stamp;
#[path = "retouch_layer_tests.rs"]
mod retouch_layers;
#[path = "file_launch_tests.rs"]
mod file_launch;
#[path = "document_tab_tests.rs"]
mod document_tabs;
#[path = "column_drop_tests.rs"]
mod column_drop;
#[path = "column_stack_tests.rs"]
mod column_stack_tests;
#[path = "paint_column_fit_tests.rs"]
mod paint_column_fit;
#[path = "contact_brush_tests.rs"]
mod contact_brush;
#[path = "squircle_tests.rs"]
mod squircle_tests;
#[path = "workspace_layout_drop_tests.rs"]
mod workspace_layout_drop;
#[path = "drag_pickup_tests.rs"]
mod drag_pickup;
#[path = "gpu_recovery_tests.rs"]
mod gpu_recovery;
#[path = "icon_tests.rs"]
mod icons;
#[path = "layer_hold_tests.rs"]
mod layer_hold;
#[path = "layer_relationship_tests.rs"]
mod layer_relationships;
#[path = "tooltip_tests.rs"]
mod tooltip;
#[path = "workspace_drawer_style_tests.rs"]
mod workspace_drawer_style;
#[path = "workspace_header_tests.rs"]
mod workspace_header;
#[path = "workspace_drag_edge_tests.rs"]
mod workspace_drag_edge;
#[path = "workspace_drop_size_tests.rs"]
mod workspace_drop_size;
#[path = "workspace_motion_tests.rs"]
mod workspace_motion;
#[path = "workspace_resize_tests.rs"]
mod workspace_resize;
#[path = "fullscreen_tests.rs"]
mod fullscreen;
#[path = "screen_view_tests.rs"]
mod screen_view;
use super::*;
use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_ui::FloatingToolbarLayout;
use std::time::{Duration, Instant};

/// A new drawing at `depth`, as New drawing makes it.
pub(crate) fn new_drawing_at(width: u32, height: u32, depth: layer_core::color::SampleDepth) -> layer_core::Document {
    NewDocumentOptions {
        extent: [width, height],
        color: layer_core::color::DocumentColor { depth, ..Default::default() },
        ..Default::default()
    }
    .project(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English))
    .unwrap()
}

fn open_native_document(input: impl std::io::Read + std::io::Seek) -> layer_core::Document {
    crate::storage::roots();
    let outcome = layer_ui::read_import(input, layer_ui::ImportIntent::Open, Default::default(), layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }, Default::default(), Default::default(), &std::sync::atomic::AtomicBool::new(false)).unwrap();
    let layer_ui::ImportOutcome::Editable(imported) = outcome else { panic!("Expected editable drawing") };
    imported.project
}
fn active_raster(document: &layer_core::Document) -> &layer_core::raster::RasterRevision {
    document.target_raster(document.working.target.unwrap()).unwrap()
}
fn active_raster_operations(document: &layer_core::Document) -> &[layer_core::RasterOperation] {
    document.target_operations(document.working.target.unwrap()).unwrap()
}
fn composition_mut(document: &mut layer_core::Document) -> &mut layer_core::authored::Composition {
    document.artwork.compositions.get_mut(document.artwork.root).unwrap()
}
fn write_capture(capture: &layer_core::authored::ArtworkCapture, output: &mut impl std::io::Write) -> Result<(), String> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    layer_core::package::codec::PreparedPackage::prepare(capture, None, &cancel)?.write(output, &cancel)
}

fn occurrence_at(document: &layer_core::Document, index: usize) -> &layer_core::authored::Occurrence {
    document.scene().occurrence(document.scene().order()[index]).unwrap()
}
fn paint_at(document: &layer_core::Document, index: usize) -> &layer_core::authored::PaintSource {
    document.scene().paint_source(document.scene().order()[index]).unwrap()
}
fn paint_at_mut(document: &mut layer_core::Document, index: usize) -> &mut layer_core::authored::PaintSource {
    let handle = match occurrence_at(document, index).content { layer_core::authored::OccurrenceContent::Paint(handle) => handle, _ => panic!("paint occurrence") };
    document.artwork.paint.get_mut(handle).unwrap()
}
fn active_occurrence(document: &layer_core::Document) -> &layer_core::authored::Occurrence {
    document.scene().occurrence(document.working.occurrence.unwrap()).unwrap()
}
fn active_paint(document: &layer_core::Document) -> &layer_core::authored::PaintSource {
    document.scene().paint_source(document.working.occurrence.unwrap()).unwrap()
}
fn active_paint_mut(document: &mut layer_core::Document) -> &mut layer_core::authored::PaintSource {
    let handle = match active_occurrence(document).content { layer_core::authored::OccurrenceContent::Paint(handle) => handle, _ => panic!("paint occurrence") };
    document.artwork.paint.get_mut(handle).unwrap()
}
fn capture_document(capture: &layer_core::authored::ArtworkCapture) -> layer_core::Document {
    layer_core::Document::from_artwork((*capture.artwork).clone()).unwrap()
}
fn write_document(document: &layer_core::Document, output: &mut impl std::io::Write) -> Result<(), String> {
    write_capture(&layer_core::Editor::new(document.clone()).capture(0, document.output().context.clone()).map_err(|e|e.to_string())?, output)
}
fn active_effect(document: &layer_core::Document) -> layer_core::EffectView<'_> {
    document.scene().effect(document.working.occurrence.unwrap()).unwrap()
}
fn artwork_manifest(document: &layer_core::Document) -> Vec<u8> {
    let capture = layer_core::Editor::new(document.clone()).capture(0, document.output().context.clone()).unwrap();
    layer_core::package::codec::PreparedPackage::prepare(&capture, None, &std::sync::atomic::AtomicBool::new(false)).unwrap().manifest().to_vec()
}
fn assert_live_artwork_eq(actual: &layer_core::Document, expected: &layer_core::Document) {
    assert_eq!(artwork_manifest(actual), artwork_manifest(expected));
}

fn assert_source_samples(actual: &layer_core::color::source::SourceImage, expected: &layer_core::color::source::SourceImage) {
    assert_eq!((actual.extent, actual.resolution, actual.interpretation.channels, actual.interpretation.depth, actual.interpretation.profile_assumed),
        (expected.extent, expected.resolution, expected.interpretation.channels, expected.interpretation.depth, expected.interpretation.profile_assumed));
    assert_eq!(layer_color::profile_bytes(&actual.interpretation.profile).unwrap(), layer_color::profile_bytes(&expected.interpretation.profile).unwrap());
    let mut actual_rows = actual.rows();
    let mut expected_rows = expected.rows();
    let mut actual_bytes = vec![0; actual.row_bytes()];
    let mut expected_bytes = vec![0; expected.row_bytes()];
    for y in 0..actual.extent[1] {
        actual_rows.read(y, &mut actual_bytes).unwrap();
        expected_rows.read(y, &mut expected_bytes).unwrap();
        assert_eq!(actual_bytes, expected_bytes);
    }
}

pub(crate) fn pump(ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    let context = glib::MainContext::default();
    while Instant::now() < until {
        while context.pending() && Instant::now() < until {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn perform(w: &Rc<Workspace>, action: layer_workspace::ManagerAction) {
    w.workspaces
        .send(w, layer_workspace::WorkspaceInput::Action { action });
    until(
        || {
            let view = w.workspaces.view();
            !view.busy && view.prompt.is_none()
        },
        "workspace action",
    );
}
pub(crate) fn until(mut ready: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "{message}");
        pump(10);
    }
}
fn wait_workspaces(w: &Workspace) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !w.workspaces.ready() || w.workspaces.busy() {
        let error = w.workspaces.manager().and_then(|m| m.error());
        assert!(Instant::now() < deadline, "workspace startup: {error:?}");
        pump(20);
    }
}
fn pen_event(camera: &layer_ui::Camera, at: [f32; 2], phase: PenPhase, sequence: u64) -> PenEvent {
    let m = camera.document_to_surface();
    PenEvent {
        device_id: 1,
        sequence,
        timestamp_ns: glib::monotonic_time() as u64 * 1000,
        view_revision: camera.revision,
        surface_position: Point {
            x: m[0] * at[0] + m[2] * at[1] + m[4],
            y: m[1] * at[0] + m[3] * at[1] + m[5],
        },
        pressure: 1.,
        tilt_radians: [0.; 2],
        twist_radians: 0.,
        distance: 0.,
        phase,
        tool: ToolKind::Pen,
        flags: SampleFlags::PRIMARY,
    }
}
fn process_memory() -> Vec<String> {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
        .map(str::to_owned)
        .collect()
}
fn ui_session(w: &Workspace) -> std::cell::Ref<'_, layer_ui::UiSession<crate::render_thread::RenderWorker>> {
    std::cell::Ref::map(w.gpu.borrow(), |gpu| &gpu.as_ref().unwrap().session)
}
fn ui_session_mut(w: &Workspace) -> std::cell::RefMut<'_, layer_ui::UiSession<crate::render_thread::RenderWorker>> {
    std::cell::RefMut::map(w.gpu.borrow_mut(), |gpu| &mut gpu.as_mut().unwrap().session)
}
fn state(w: &Workspace) -> UiState {
    ui_session(w).state().clone()
}
fn drag_divider(w: &Rc<Workspace>, id: u32, to: [f32; 2]) {
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let d = w.resolved().dividers.into_iter().find(|d| d.id == id).unwrap();
    let from = [
        d.bounds.x + d.bounds.width * 0.5,
        d.bounds.y + d.bounds.height * 0.5,
    ];
    for (phase, position) in [
        (ContactPhase::Down, from),
        (ContactPhase::Move, to),
        (ContactPhase::Up, to),
    ] {
        w.dispatch(UiAction::DragDivider {
            id,
            phase,
            position,
            viewport,
        });
    }
}

// Dock/gesture regressions exercise a stable, deliberately customized workspace
// (including its tab IDs and eight-tile ribbon), not the evolving shipped preset.
// The default-workspace integration test uses the actual startup path.
fn apply_fixture_theme(w: &Rc<Workspace>) {
    if let Ok(theme) = std::env::var("CAPY_NATIVE_TEST_THEME") {
        let theme = match theme.as_str() {
            "light" => layer_ui::Theme::Light,
            "dark" => layer_ui::Theme::Dark,
            _ => panic!("CAPY_NATIVE_TEST_THEME must be light or dark"),
        };
        if w.gpu.borrow().is_none() {
            w.dispatch(UiAction::RestoreSettings { settings: layer_ui::Settings {
                theme: Some(theme), ..Default::default()
            }});
        } else {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        }
    }
}
fn fixture_workspace(app: &adw::Application) -> Rc<Workspace> {
    let w = Workspace::new(app);
    apply_fixture_theme(&w);
    w.area.connect_realize(glib::clone!(
        #[weak]
        w,
        move |_| w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState::default()),
        })
    ));
    w
}

fn artifact_dir(path: &str) -> &str {
    std::fs::create_dir_all(path).unwrap();
    path
}

pub(crate) fn save_snapshot(w: &Workspace, wait: u64, path: impl FnOnce() -> std::path::PathBuf) {
    let _ = crate::snapshot(w);
    pump(wait);
    crate::snapshot(w).save_to_png(path()).unwrap();
}

fn tool_settings_workspace(w: &Rc<Workspace>, commands: &[CommandId], hide_sizes: bool, below_brushes: bool) -> WorkspaceState {
    let mut workspace = state(w).workspace;
    if hide_sizes { workspace.layout.set_panel_visible(Panel::Sizes, false).unwrap(); }
    if !commands.is_empty() {
        let controls: Vec<_> = commands.iter().map(|&command| ToolbarControl::Command { command }).collect();
        workspace.layout.insert_tools(Panel::Toolbar, None, &controls).unwrap();
    }
    workspace.layout.set_panel_visible(Panel::ToolSettings, true).unwrap();
    if below_brushes {
        let group = workspace.layout.panel_group(Panel::Brushes).unwrap();
        workspace.layout.move_panel([w.surface.width() as f32, w.surface.height() as f32],
            Panel::ToolSettings, DockTarget::Split { group, edge: Edge::Bottom }).unwrap();
    }
    workspace
}

#[test]
#[ignore = "private Wayland display and GPU: shipped editor preset"]
fn native_default_workspace() {
    let app = native_test_app("art.capycanvas.DefaultWorkspace");
    let w = Workspace::new(&app);
    w.window.present();
    pump(1600);
    until(
        || {
            ui_session(&w)
                .engine()
                .backend()
                .startup
                .brush_ready
        },
        "active brush startup",
    );
    for (row, color) in [
        [0.12, 0.38, 0.58, 1.],
        [0.8, 0.4, 0.22, 1.],
        [0.2, 0.6, 0.43, 1.],
    ]
    .into_iter()
    .enumerate()
    {
        w.dispatch(UiAction::SetColor { rgba: color });
        w.dispatch(UiAction::SetBrushSize {
            value: 42. + row as f32 * 12.,
        });
        let points: Vec<_> = (0..24)
            .map(|i| {
                let t = i as f32 / 23.;
                [
                    350. + 1320. * t,
                    510. + row as f32 * 230. - (t * std::f32::consts::PI * 2.).sin() * 90.,
                ]
            })
            .collect();
        native_pen_path(&w, &points);
    }
    assert_eq!(
        ui_session(&w)
            .engine()
            .metrics()
            .committed_strokes,
        3
    );
    let output = artifact_dir("../../artifacts/familiar-workspace/default");
    let initial = state(&w).workspace.layout;
    assert_eq!(initial.bands, DockLayout::editor_default().bands);
    assert_eq!(initial.panels, DockLayout::editor_default().panels);
    let verify = || {
        let state = state(&w);
        let layout = ui_session(&w)
            .layout([w.surface.width() as f32, w.surface.height() as f32]);
        assert!(layout.work_area.width >= 64. && layout.work_area.height > 0.);
        for g in &layout.groups {
            let native = w
                .groups
                .borrow()
                .iter()
                .find(|view| view.id == g.id)
                .unwrap()
                .stack
                .parent()
                .unwrap();
            let b = native.compute_bounds(&w.surface).unwrap();
            for (actual, expected) in [
                (b.x(), g.bounds.x),
                (b.y(), g.bounds.y),
                (b.width(), g.bounds.width),
                (b.height(), g.bounds.height),
            ] {
                assert!(
                    (actual - expected).abs() < 1.1,
                    "{:?}: {b:?} {:?}",
                    g.active,
                    g.bounds
                );
            }
            if let Some(tiles) = &g.tiles {
                let widget = w.panel_widget(g.active);
                for (tile, bounds) in state
                    .workspace
                    .layout
                    .panel(g.active)
                    .unwrap()
                    .tiles()
                    .iter()
                    .zip(&tiles.tiles)
                {
                    let button = find_named(&widget, &format!("tile-{}", tile.id)).unwrap();
                    let b = button.compute_bounds(&widget).unwrap();
                    assert!((b.x() - bounds.x).abs() < 1.1 && (b.y() - bounds.y).abs() < 1.1);
                    assert!(
                        (b.width() - bounds.width).abs() < 1.1
                            && (b.height() - bounds.height).abs() < 1.1
                    );
                    assert!(b.x() + b.width() <= widget.width() as f32 + 1.);
                    assert!(b.y() + b.height() <= widget.height() as f32 + 1.);
                    if tile.control != ToolbarControl::Divider {
                        assert!(button.tooltip_text().is_some());
                    }
                }
            }
        }
        assert!(!w.status.is_visible(), "{}", w.status.text());
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        // The resized wheel is centered; native contacts must use that same
        // origin, and all color controls remain visible at the default height.
        pump(100);
        for panel in [
            Panel::Sizes,
            Panel::ToolSettings,
            Panel::Stats,
            Panel::Navigator,
        ] {
            let tab = w.groups.borrow().iter().flat_map(|g| &g.tabs)
                .find(|(p, _)| *p == panel).unwrap().1.clone();
            click(&tab);
            pump(150);
            assert!(w.panel_widget(panel).is_mapped());
            let other = match panel {
                Panel::Sizes => Panel::ToolSettings,
                Panel::ToolSettings => Panel::Sizes,
                Panel::Stats => Panel::Navigator,
                _ => Panel::Stats,
            };
            assert!(!w.panel_widget(other).is_mapped());
            assert!(state(&w).customization.drawer.is_none());
            verify();
            if panel == Panel::Sizes {
                assert!(!w.size_number.is_mapped());
                let buttons = w.size_buttons.borrow();
                assert_eq!(buttons.len(), 40);
                let first = buttons[0].1.compute_bounds(&w.panel_widget(panel)).unwrap();
                for (_, button) in &buttons[..6] {
                    let bounds = button.compute_bounds(&w.panel_widget(panel)).unwrap();
                    assert_eq!(bounds.y(), first.y());
                    assert_eq!(bounds.height(), layer_ui::BRUSH_SIZE_TILE[1]);
                }
                for (value, button) in buttons.iter().filter(|(value, _)| [0.7, 1.5, 2.5, 2000.].contains(value)) {
                    click(button); assert_eq!(state(&w).brush.diameter, *value);
                }
                capture_reference(&w, &format!("{output}/brush-size-tab-{theme:?}.png"), 1.);
            }
            if panel == Panel::Stats {
                pump(250);
                let view = ui_session(&w).renderer_stats();
                let mut expected: Vec<_> = view.rows.iter().map(|row| row.label).collect();
                expected.insert(view.chart_after_rows, "chart");
                expected.push("Start stroke recording");
                let mut actual = Vec::new();
                let mut child = w.effects.stats.first_child();
                while let Some(widget) = child {
                    actual.push(if widget.is::<gtk::DrawingArea>() {
                        "chart".to_string()
                    } else {
                        widget
                            .first_child()
                            .unwrap()
                            .downcast::<gtk::Label>()
                            .unwrap()
                            .text()
                            .to_string()
                    });
                    child = widget.next_sibling();
                }
                assert_eq!(actual, expected);
                capture_reference(&w, &format!("{output}/diagnostics-tab-{theme:?}.png"), 1.);
            }
        }
        let wheel = find_named(&w.panel_widget(Panel::Color), "color-wheel").unwrap();
        let (size, origin) = wheel
            .clone()
            .downcast::<crate::tool_panels::ColorWheel>()
            .unwrap()
            .drawing_bounds();
        assert!(size >= 92.);
        let point = state(&w)
            .colors
            .wheel_hue_marker(&layer_ui::ColorWheelGeometry::new(size).unwrap(), 210.);
        let controllers = wheel.observe_controllers();
        let drag = (0..controllers.n_items())
            .find_map(|i| controllers.item(i).and_downcast::<gtk::GestureDrag>())
            .unwrap();
        drag.emit_by_name::<()>(
            "drag-begin",
            &[
                &((point[0] + origin[0]) as f64),
                &((point[1] + origin[1]) as f64),
            ],
        );
        // Circle uses its own ring rotation and Okhsv, not the legacy HSB readout.
        assert!(
            (state(&w).colors.wheel_components()[0] - 210.).abs() < 0.01,
            "hue at the visible ring marker: {:?}",
            state(&w).colors.wheel_components()
        );
        let viewport = w.panel_widget(Panel::Color);
        let component = find_named(&viewport, "color-readout")
            .unwrap()
            .compute_bounds(&viewport)
            .unwrap();
        assert!(component.y() + component.height() <= viewport.height() as f32);
        for (tile, id) in initial
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()
            .iter()
            .filter_map(|tile| {
                if let ToolbarControl::Command { command } = tile.control {
                    Some((tile.id, command))
                } else {
                    None
                }
            })
        {
            click(&command(&w, id));
            if state(&w).customization.drawer.is_some() {
                w.dispatch(UiAction::Customize {
                    action: CustomizationAction::CloseExpanded,
                });
            }
            pump(230);
            assert!(
                ui_session(&w)
                    .command(id)
                    .selected,
                "{id:?}"
            );
            verify();
            capture_reference(&w, &format!("{output}/{id:?}-{theme:?}.png"), 1.);
            let picker = id == CommandId::Eyedropper;
            if picker {
                w.dispatch(UiAction::ColorPicker {
                    action: layer_ui::ColorPickerAction::Settings {
                        anchor: layer_ui::DrawerAnchor::Tile {
                            panel: Panel::Toolbar,
                            tile,
                        },
                    },
                });
            } else {
                click(&command(&w, id));
            }
            pump(250);
            assert!(state(&w).customization.drawer.is_some(), "{id:?} controls");
            assert_drawer_connected(&w);
            if picker {
                w.dispatch(UiAction::Customize {
                    action: CustomizationAction::CloseExpanded,
                });
            }
            click(&command(&w, id));
            pump(250);
            assert!(state(&w).customization.drawer.is_none());
        }
        let commands = w.panel_widget(Panel::Commands);
        for id in [
            CommandId::NewDocument,
            CommandId::OpenDocument,
            CommandId::SaveDocument,
            CommandId::Undo,
            CommandId::Redo,
            CommandId::ClearLayer,
            CommandId::FillSelection,
            CommandId::ScaleRotate,
            CommandId::FlipHorizontal,
        ] {
            let tile = state(&w)
                .workspace
                .layout
                .panel(Panel::Commands)
                .unwrap()
                .tiles()
                .iter()
                .find(|t| t.control == (ToolbarControl::Command { command: id }))
                .unwrap()
                .id;
            let button = find_named(&commands, &format!("tile-{tile}")).unwrap();
            assert_eq!(
                button.is_sensitive(),
                ui_session(&w).command(id).enabled
            );
        }
        let flipped = state(&w).camera;
        click(&command(&w, CommandId::FlipHorizontal));
        pump(100);
        assert_ne!(state(&w).camera, flipped);
        click(&command(&w, CommandId::FlipHorizontal));
        w.dispatch(UiAction::Invoke {
            command: CommandId::Brush,
        });
        w.window.unmaximize();
        pump(200);
        w.window.set_default_size(900, 640);
        pump(500);
        verify();
        for id in [CommandId::ZoomIn, CommandId::FlipVertical] {
            let button =
                find_named(w.navigator.root.upcast_ref(), &format!("navigator-{id:?}")).unwrap();
            let b = button.compute_bounds(&w.navigator.root).unwrap();
            assert!(b.y() >= 0. && b.y() + b.height() <= w.navigator.root.height() as f32);
            let bounds = button.compute_bounds(&w.surface).unwrap();
            let picked = w
                .surface
                .pick(
                    (bounds.x() + bounds.width() * 0.5) as f64,
                    (bounds.y() + bounds.height() * 0.5) as f64,
                    gtk::PickFlags::DEFAULT,
                )
                .unwrap();
            assert!(picked == button || picked.is_ancestor(&button));
        }
        capture_reference(&w, &format!("{output}/compact-{theme:?}.png"), 1.);
        w.window.set_default_size(1200, 900);
        pump(400);
    }
    w.window.destroy();
    pump(80);
}

#[test]
#[ignore = "native file workflow: private Wayland display and GPU"]
#[allow(deprecated)] // Inspect GtkFileDialog's fallback widget, not a production API.
fn native_document_files() {
    // Application::run normally sets argv[0]; this registered test app has none.
    glib::set_prgname(Some("capy-canvas-test"));
    glib::set_application_name(APP_NAME);
    let app = native_test_app("art.capycanvas.DocumentFiles");
    let output = std::path::Path::new("../../artifacts/familiar-workspace/files");
    std::fs::create_dir_all(output).unwrap();
    let output = output.canonicalize().unwrap();
    let path = output.join("drawing.capy");
    let location = DocumentLocation {
        uri: gtk::gio::File::for_path(&path).uri().into(),
        name: "drawing.capy".into(),
    };
    let w = Workspace::with_project(
        &app,
        Some((new_drawing(384, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), Some(location.clone()))),
    );
    apply_fixture_theme(&w);
    let created = Rc::new(RefCell::new(None));
    let result = created.clone();
    *w.open_document.borrow_mut() = Some(Rc::new(move |project, location| {
        *result.borrow_mut() = Some((project, location))
    }));
    w.window.present();
    let ready = |w: &Rc<Workspace>| {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            pump(20);
            if w.gpu.borrow().as_ref().is_some_and(|g| {
                g.session.engine().backend().startup.complete
                    && !g.session.state().filter_load.pending
                    && !g.session.engine().has_pending_document_edits()
            }) {
                return;
            }
        }
        panic!("document did not become ready");
    };
    ready(&w);
    let image = layer_core::color::source::rgba8_source([96, 64], |x, y| {
        if (y / 8 + x / 8) % 2 == 0 {
            [235, 60, 90, 180]
        } else {
            [25, 160, 220, 95]
        }
    });
    ui_session_mut(&w)
        .import_layer_source("Imported color", std::sync::Arc::unwrap_or_clone(image))
        .unwrap();
    w.refresh(regions::DOCUMENT | regions::COMMANDS);
    w.wake();
    ready(&w);
    assert!(state(&w).document_file.modified);
    // Autosave publishes a separate durable copy without acknowledging Save.
    w.recovery().capture(&w);
    glib::MainContext::default().block_on(w.recovery().drain());
    let recovery_path=w.recovery().published_path();
    let recovery=glib::MainContext::default().block_on(w.recovery().read_snapshot()).unwrap().document().clone();
    let sources = |p: &layer_core::Document| p.artwork.paint.iter().filter(|(_, _, source)| source.base.is_some()).count();
    assert_eq!(sources(&recovery), 1);
    assert!(state(&w).document_file.modified);
    w.dispatch(UiAction::Invoke {
        command: CommandId::SaveDocument,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while state(&w).document_file.busy && Instant::now() < deadline {
        pump(20);
    }
    assert!(!state(&w).document_file.busy);
    assert!(
        !state(&w).document_file.modified,
        "{:?}",
        state(&w).host_error
    );
    w.recovery().capture(&w);
    glib::MainContext::default().block_on(w.recovery().drain());
    assert!(recovery_path.join("head.json").exists());
    let saved_session=glib::MainContext::default().block_on(w.recovery().read_snapshot()).unwrap();
    assert_eq!(saved_session.state.saved_checkpoint,saved_session.editor.checkpoint());
    let bytes=std::sync::Arc::<[u8]>::from(std::fs::read(&path).unwrap());
    let backing=layer_core::package::ImmutableBacking::new(std::sync::Arc::new(bytes)).unwrap();
    let opened=layer_core::package::codec::open(backing,Default::default(),&std::sync::atomic::AtomicBool::new(false)).unwrap();
    assert!(matches!(opened,layer_core::package::codec::OpenOutcome::Candidate {preview:Some(ref p),..} if p.size()==[384,256]));
    let project =
        open_native_document(std::fs::File::open(&path).unwrap());
    assert_eq!(sources(&project), 1);
    let before = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 900))
        .unwrap();
    assert_eq!([before.width, before.height], [384, 256]);
    // A native surface/device replacement retains saved identity, exact raster
    // and undo roots. Hiding/unrealizing the picture exercises the GTK signals.
    let checkpoint = ui_session(&w)
        .engine()
        .checkpoint();
    w.area.set_visible(false);
    pump(30);
    w.area.unrealize();
    assert!(
        w.gpu.borrow().is_some(),
        "surface teardown retains the session"
    );
    w.area.set_visible(true);
    ready(&w);
    let restored = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 901))
        .unwrap();
    assert_eq!(restored.bytes, before.bytes);
    assert_eq!(
        ui_session(&w)
            .engine()
            .checkpoint(),
        checkpoint
    );
    assert_eq!(state(&w).document_file.location, Some(location.clone()));
    assert!(!state(&w).document_file.modified);
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    w.wake();
    ready(&w);
    assert!(state(&w).document_file.modified);
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    w.wake();
    ready(&w);
    assert!(!state(&w).document_file.modified);
    // GDK_DEBUG=no-portals selects GTK's chooser fallback in this isolated
    // display; production keeps GtkFileDialog's normal portal selection.
    let chooser = || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            pump(30);
            if let Some(dialog) = gtk::Window::list_toplevels()
                .into_iter()
                .find_map(|w| w.downcast::<gtk::FileChooserDialog>().ok())
                .filter(|d| d.is_visible())
            {
                // Allow the native folder/path-bar model to finish opening
                // before sending synthetic chooser responses.
                pump(500);
                return dialog;
            }
            assert!(
                Instant::now() < deadline,
                "native file chooser was not shown"
            );
        }
    };
    let finish = || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let complete = !w.servicing.get()
                && !w.documents.has_pending_open()
                && !w.documents.changing.get()
                && !w.documents.paused.get()
                && w.gpu.borrow().as_ref().is_some_and(|g| !g.session.state().document_file.busy);
            if complete { break; }
            assert!(Instant::now() < deadline,
                "file operation did not finish: servicing={} pending_open={} changing={} paused={} renderer={}",
                w.servicing.get(), w.documents.has_pending_open(), w.documents.changing.get(),
                w.documents.paused.get(), w.gpu.borrow().is_some());
            pump(30);
        }
    };
    let original_tab = w.documents.selected();
    let original_count = w.documents.len();
    w.dispatch(UiAction::Invoke {
        command: CommandId::OpenDocument,
    });
    chooser().response(gtk::ResponseType::Cancel);
    finish();
    assert!(created.borrow().is_none());
    assert_eq!(w.documents.len(), original_count);
    assert_eq!(w.documents.selected(), original_tab);
    assert!(state(&w).host_error.is_none());
    w.dispatch(UiAction::Invoke {
        command: CommandId::OpenDocument,
    });
    let open = chooser();
    open.set_file(&gtk::gio::File::for_path(&path)).unwrap();
    pump(250);
    open.response(gtk::ResponseType::Accept);
    finish();
    new_photo::ready(&w);
    assert_eq!(w.documents.len(), original_count + 1);
    assert_ne!(w.documents.selected(), original_tab);
    assert!(created.borrow().is_none());
    let opened_tab = w.documents.selected();
    let opened = ui_session(&w).engine().document().clone();
    let origin = state(&w).document_file.location;
    let mut opened_bytes = Vec::new();
    let mut project_bytes = Vec::new();
    write_capture(&layer_host::tasks::capture_document(&opened), &mut opened_bytes).unwrap();
    write_capture(&layer_host::tasks::capture_document(&project), &mut project_bytes).unwrap();
    assert_eq!(opened_bytes, project_bytes);
    assert_eq!(origin, Some(location.clone()));
    assert!(!state(&w).document_file.modified);
    w.documents.select(&w, opened_tab, true);
    until(|| w.documents.len() == original_count && w.documents.selected() == original_tab
        && !w.documents.changing.get(), "opened drawing closes and restores original tab");
    new_photo::ready(&w);
    assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
    assert_eq!(state(&w).document_file.location, Some(location.clone()));
    assert!(!state(&w).document_file.modified);
    let invalid = output.join("invalid.capy");
    std::fs::write(&invalid, b"not a project").unwrap();
    w.dispatch(UiAction::Invoke {
        command: CommandId::OpenDocument,
    });
    let open = chooser();
    open.set_file(&gtk::gio::File::for_path(invalid)).unwrap();
    pump(250);
    open.response(gtk::ResponseType::Accept);
    finish();
    assert!(created.borrow().is_none());
    assert_eq!(w.documents.len(), original_count);
    assert_eq!(w.documents.selected(), original_tab);
    assert!(state(&w).host_error.is_some());
    w.wake();
    pump(80);
    assert!(w.status.is_visible());
    let save_path = output.join(format!("copy-{}.capy", std::process::id()));
    let png_path = output.join(format!("export-{}.png", std::process::id()));
    let tiff_path = output.join(format!("export-{}.tif", std::process::id()));
    let jpeg_path = output.join(format!("export-{}.jpg", std::process::id()));
    let mut custom_exports = Vec::new();
    for (name, extension, profile, channels) in [
        ("rgb", "tif", layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::AdobeRgb), layer_core::color::source::SourceChannels::Rgba),
        ("gray", "png", layer_color::gray_profile(layer_core::color::RgbSpace::ProPhoto).unwrap(), layer_core::color::source::SourceChannels::GrayAlpha),
    ] {
        let profile_path = output.join(format!("delivery-{name}.icc"));
        let bytes = layer_color::profile_bytes(&profile).unwrap();
        std::fs::write(&profile_path, &bytes).unwrap();
        custom_exports.push((output.join(format!("custom-{name}-{}.{}", std::process::id(), extension)), profile_path, bytes, channels));
    }
    if let Some(path) = std::env::var_os("LAYER_TEST_CMYK_PROFILE") {
        let path = std::path::PathBuf::from(path);
        let bytes = std::fs::read(&path).unwrap();
        custom_exports.push((output.join(format!("custom-cmyk-{}.tif", std::process::id())), path, bytes, layer_core::color::source::SourceChannels::Cmyk));
    }
    let bad_profile = output.join("invalid-profile.icc");
    std::fs::write(&bad_profile, b"invalid ICC profile").unwrap();
    let mut deliveries = vec![
        (CommandId::SaveDocumentAs, &save_path),
        (CommandId::ExportDocument, &png_path),
        (CommandId::ExportDocument, &tiff_path),
        (CommandId::ExportDocument, &jpeg_path),
    ];
    deliveries.extend(custom_exports.iter().map(|(path, _, _, _)| (CommandId::ExportDocument, path)));
    for (command, path) in deliveries {
        w.dispatch(UiAction::Invoke { command });
        if command == CommandId::ExportDocument {
            pump(200);
            let options = w.window.visible_dialog().unwrap();
            // This codec matrix explicitly starts from the original Web choices.
            // Successful delivery now remembers the destination between sheets.
            new_photo::export_page(&w, "presets");
            let reset = named::<adw::ButtonRow>(options.upcast_ref(), "export-preset-reset");
            reset.emit_by_name::<()>("activated", &[]);
            pump(100);
            until(|| reset.is_sensitive(), "reset export destination");
            for (name, selected) in [
                ("export-preset", "Web / Share"), ("export-format", "PNG"),
                ("export-depth", "8-bit"),
                ("export-background", "Keep transparency"),
            ] {
                let row = named::<adw::ComboRow>(options.upcast_ref(), name);
                assert_eq!(row.subtitle().as_deref(), Some(selected), "{name}");
            }
            if path == &tiff_path {
                named::<adw::ComboRow>(options.upcast_ref(), "export-preset").set_selected(2);
                assert_eq!(named::<adw::ComboRow>(options.upcast_ref(), "export-format").selected(), 1);
                assert_eq!(named::<adw::ComboRow>(options.upcast_ref(), "export-depth").selected(), 1);
                assert!(!named::<adw::SwitchRow>(options.upcast_ref(), "export-dither").is_visible());
                new_photo::profile_action(&w, "export", "builtin-3");
            }
            if path == &jpeg_path {
                named::<adw::ComboRow>(options.upcast_ref(), "export-format").set_selected(2);
                let depth = named::<adw::ComboRow>(options.upcast_ref(), "export-depth");
                assert_eq!(depth.selected(), 0);
                assert!(!depth.is_sensitive());
                let background = named::<adw::ComboRow>(options.upcast_ref(), "export-background");
                assert_eq!(background.selected(), 0);
                assert_eq!(background.model().unwrap().n_items(), 2);
                background.set_selected(1);
                assert!(new_photo::export_enabled(&w));
                new_photo::profile_action(&w, "export", "builtin-1");
                let quality = named::<crate::number_control::NumberControl>(options.upcast_ref(), "export-jpeg-quality");
                assert!(quality.is_visible());
                descendant::<gtk::Stack>(&quality).unwrap().set_visible_child_name("entry");
                descendant::<gtk::Entry>(&quality).unwrap().set_text("95");
                assert!(quality.commit_text());
                assert_eq!(quality.value(), 95.);
                let advanced = named::<adw::ExpanderRow>(options.upcast_ref(), "export-advanced");
                assert!(!advanced.is_expanded());
                advanced.set_expanded(true);
                let intent = named::<adw::ComboRow>(options.upcast_ref(), "export-intent");
                let dither = named::<adw::SwitchRow>(options.upcast_ref(), "export-dither");
                assert_eq!(intent.selected(), 0);
                assert!(!dither.is_active());
                dither.set_active(true);
                pump(350);
                let scroll = named::<gtk::ScrolledWindow>(options.upcast_ref(), "export-color-scroll");
                let adjustment = scroll.vadjustment();
                adjustment.set_value(adjustment.upper() - adjustment.page_size());
                pump(80);
                capture_reference(&w, output.join("export-advanced-options.png").to_str().unwrap(), 1.);
                advanced.set_expanded(false);
                pump(350); // Capture the settled native expander, not its animation.
                adjustment.set_value(0.);
            }
            if let Some((_, profile_path, _, channels)) = custom_exports.iter().find(|(p, _, _, _)| p == path) {
                let export_enabled = || new_photo::export_enabled(&w);
                let button = named::<gtk::MenuButton>(options.upcast_ref(), "export-profile-choose");
                let wait_profile = || {
                    let deadline = Instant::now() + Duration::from_secs(15);
                    while Instant::now() < deadline && !button.is_sensitive() { pump(5); }
                    assert!(button.is_sensitive(), "ICC file worker did not finish");
                };
                // Cancellation and invalid metadata retain the sheet and do not
                // enable delivery with a silently assumed profile.
                new_photo::profile_action(&w, "export", "add");
                chooser().response(gtk::ResponseType::Cancel);
                wait_profile();
                assert!(export_enabled());
                new_photo::profile_action(&w, "export", "add");
                let file = chooser();
                file.set_file(&gtk::gio::File::for_path(&bad_profile)).unwrap();
                pump(250);
                file.response(gtk::ResponseType::Accept);
                wait_profile();
                assert!(find_named(options.upcast_ref(), "export-profile-error").unwrap().is_visible());
                assert!(!export_enabled());
                new_photo::profile_action(&w, "export", "add");
                let file = chooser();
                file.set_file(&gtk::gio::File::for_path(profile_path)).unwrap();
                pump(250);
                file.response(gtk::ResponseType::Accept);
                wait_profile();
                assert!(!find_named(options.upcast_ref(), "export-profile-error").unwrap().is_visible());
                new_photo::profile_action(&w, "export", "add");
                chooser().response(gtk::ResponseType::Cancel);
                wait_profile();
                assert!(export_enabled());
                let format = named::<adw::ComboRow>(options.upcast_ref(), "export-format");
                let background = named::<adw::ComboRow>(options.upcast_ref(), "export-background");
                if *channels == layer_core::color::source::SourceChannels::Cmyk {
                    assert_eq!(format.selected(), 1);
                    assert_eq!(background.selected(), 0);
                    assert_eq!(background.model().unwrap().n_items(), 2);
                    format.set_selected(0);
                    assert!(!export_enabled());
                    format.set_selected(1);
                    assert_eq!(background.model().unwrap().n_items(), 2);
                    background.set_selected(1);
                } else {
                    format.set_selected(u32::from(path.extension().unwrap() == "tif"));
                }
                named::<adw::ComboRow>(options.upcast_ref(), "export-depth").set_selected(1);
                assert!(export_enabled());
                if *channels == layer_core::color::source::SourceChannels::Rgba {
                    // Selected bytes are frozen; changing the file afterwards
                    // cannot retag or change the output at publication time.
                    std::fs::write(profile_path, b"profile changed after selection").unwrap();
                }
                pump(250);
                capture_reference(&w, output.join(format!("export-custom-{:?}-options.png", channels)).to_str().unwrap(), 1.);
            }
            pump(80);
            capture_reference(&w, output.join(if path == &tiff_path { "export-tiff-options.png" } else if path == &jpeg_path { "export-jpeg-options.png" } else { "export-options.png" }).to_str().unwrap(), 1.);
            new_photo::response(&w, "export");
        }
        let save = chooser();
        assert_eq!(
            save.current_folder().unwrap().uri(),
            gtk::gio::File::for_path(&output).uri()
        );
        save.set_current_name(path.file_name().unwrap().to_str().unwrap());
        pump(300);
        save.response(gtk::ResponseType::Accept);
        finish();
        assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
        assert!(path.is_file());
    }
    for (path, _, profile, channels) in &custom_exports {
        let decoded = layer_color::photo::read_photo(std::io::BufReader::new(std::fs::File::open(path).unwrap()), Default::default()).unwrap();
        assert_eq!(decoded.extent, [384, 256]);
        assert_eq!(decoded.interpretation.channels, *channels);
        assert_eq!(decoded.interpretation.depth, layer_core::color::SampleDepth::U16);
        assert_eq!(layer_color::profile_bytes(&decoded.interpretation.profile).unwrap(), *profile);
    }
    let tiff = layer_color::photo::read_photo(std::io::BufReader::new(std::fs::File::open(&tiff_path).unwrap()), Default::default()).unwrap();
    assert_eq!(tiff.extent, [384, 256]);
    assert_eq!(tiff.interpretation.depth, layer_core::color::SampleDepth::U16);
    assert_eq!(layer_color::profile_bytes(&tiff.interpretation.profile).unwrap(),
        layer_color::profile_bytes(&layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::ProPhoto)).unwrap());
    let jpeg = layer_color::photo::read_photo(std::io::BufReader::new(std::fs::File::open(&jpeg_path).unwrap()), Default::default()).unwrap();
    assert_eq!(jpeg.extent, [384, 256]);
    assert_eq!(jpeg.interpretation.depth, layer_core::color::SampleDepth::U8);
    assert_eq!(jpeg.interpretation.channels, layer_core::color::source::SourceChannels::Rgb);
    assert_eq!(layer_color::profile_bytes(&jpeg.interpretation.profile).unwrap(),
        layer_color::profile_bytes(&layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::DisplayP3)).unwrap());
    let mut png = png::Decoder::new(std::fs::File::open(png_path).unwrap())
        .read_info()
        .unwrap();
    let mut pixels = vec![0; png.output_buffer_size()];
    let frame = png.next_frame(&mut pixels).unwrap();
    assert_eq!(&pixels[..frame.buffer_size()], &before.bytes);
    assert_eq!(
        state(&w).document_file.location.as_ref().unwrap().uri,
        gtk::gio::File::for_path(&save_path).uri()
    );
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::Invoke {
            command: CommandId::NewDocument,
        });
        pump(220);
        assert!(state(&w).document_file.busy);
        let width = named::<adw::SpinRow>(w.window.upcast_ref(), "new-document-width");
        width.set_value(512.);
        capture_reference(
            &w,
            output.join(format!("new-{theme:?}.png")).to_str().unwrap(),
            1.,
        );
        click(&find_button(w.window.upcast_ref(), "Create").unwrap());
        pump(220);
        let (project, location) = created.borrow_mut().take().unwrap();
        assert_eq!(project.composition().size[0], 512);
        assert!(location.is_none());
        assert!(!state(&w).document_file.busy);
        assert_eq!(state(&w).tabs[0].width, 384);
    }
    w.dispatch(UiAction::Invoke {
        command: CommandId::NewDocument,
    });
    pump(200);
    // The title-bar customization bank also owns a hidden Cancel button.
    // Target the current native dialog rather than the first label in the window.
    let dialog = w.window.visible_dialog().unwrap();
    click(&find_button(dialog.upcast_ref(), &layer_ui::new_document_spec(&w.localization()).cancel).unwrap());
    finish();
    assert!(created.borrow().is_none());
    let reopened = Workspace::with_project(&app, Some((project, Some(location))));
    reopened.window.present();
    // Match the application factory's settings inheritance for a new window.
    reopened.dispatch(UiAction::RestoreSettings {
        settings: state(&w).settings,
    });
    ready(&reopened);
    let after = glib::MainContext::default()
        .block_on(read_canvas_pixels(&reopened, 901))
        .unwrap();
    assert_eq!(before.bytes, after.bytes);
    assert!(!state(&reopened).document_file.modified);
    capture_reference(&reopened, output.join("reopened.png").to_str().unwrap(), 1.);
    reopened.window.close();
    pump(200);
    assert!(!reopened.window.is_visible());
    w.dispatch(UiAction::Invoke {
        command: CommandId::AddLayer,
    });
    assert!(state(&w).document_file.modified);
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        until(||!w.servicing.get()&&w.window.visible_dialog().is_none(),"close dialog ready");
        w.documents.select(&w,w.documents.selected(),true);
        until(||w.window.visible_dialog().is_some(),"dirty tab close asks");
        assert!(w.window.is_visible());
        capture_reference(
            &w,
            output
                .join(format!("unsaved-{theme:?}.png"))
                .to_str()
                .unwrap(),
            1.,
        );
        let dialog=w.window.visible_dialog().unwrap();
        click(&find_button(dialog.upcast_ref(), &layer_ui::new_document_spec(&w.localization()).cancel).unwrap());
        until(||!w.servicing.get()&&w.window.visible_dialog().is_none(),"tab close cancelled");
        assert!(w.window.is_visible());
        assert!(state(&w).document_file.modified);
    }
    until(||!w.servicing.get()&&w.window.visible_dialog().is_none(),"previous dialog closed");
    w.documents.select(&w,w.documents.selected(),true);
    until(||w.window.visible_dialog().is_some_and(|dialog|dialog.is_mapped()),"explicit close asks to save");
    let dialog=w.window.visible_dialog().unwrap();
    click(&find_button(dialog.upcast_ref(), "Save").unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    while w.window.is_visible() && Instant::now() < deadline {
        pump(20);
    }
    assert!(!w.window.is_visible(),"{:?}; {:?}",state(&w).document_file,state(&w).host_error);
    let saved =
        open_native_document(std::fs::File::open(save_path).unwrap());
    assert_eq!(saved.scene().order().len(), 4);
}

#[test]
#[ignore = "private Wayland desktop: startup timing and event-loop responsiveness"]
fn native_startup_latency() {
    let app = native_test_app("art.capycanvas.StartupTest");
    let started = Instant::now();
    let w = Workspace::new(&app);
    let built = started.elapsed().as_secs_f64() * 1000.;
    w.window.present();
    let presented = started.elapsed().as_secs_f64() * 1000.;
    let send = |phase, x| {
        let now = glib::monotonic_time() as u64 * 1000;
        let event = pen_event(&state(&w).camera, [x, 400.], phase, now);
        w.input.send(
            &w,
            PenEvent {
                device_id: 92,
                timestamp_ns: now,
                ..event
            },
        );
    };
    let strokes = || {
        ui_session(&w)
            .engine()
            .metrics()
            .committed_strokes
    };
    send(PenPhase::Down, 400.);
    w.dispatch(UiAction::SetBrushSize { value: 24. });
    let mut stages = [None; 3];
    let mut first_canvas = None;
    let mut painted = false;
    let mut max_pump_ms = 0f64;
    while stages[2].is_none() || first_canvas.is_none() {
        let tick = Instant::now();
        pump(1);
        max_pump_ms = max_pump_ms.max(tick.elapsed().as_secs_f64() * 1000.);
        if first_canvas.is_none()
            && w.gpu.borrow().as_ref().is_some_and(|g| {
                !g.session
                    .engine()
                    .backend()
                    .stats
                    .lock()
                    .unwrap()
                    .presented
                    .is_empty()
            })
        {
            first_canvas = Some(started.elapsed().as_secs_f64() * 1000.);
        }
        let progress = ui_session(&w)
            .engine()
            .backend()
            .startup;
        for (slot, ready) in stages.iter_mut().zip([
            progress.canvas_ready,
            progress.brush_ready,
            progress.complete,
        ]) {
            if ready && slot.is_none() {
                *slot = Some(started.elapsed().as_secs_f64() * 1000.);
            }
        }
        if progress.brush_ready && !painted {
            // A contact that began before readiness replays as one whole stroke.
            send(PenPhase::Move, 450.);
            send(PenPhase::Up, 500.);
            // A new contact works without waiting for the unused catalog.
            send(PenPhase::Down, 400.);
            send(PenPhase::Move, 450.);
            send(PenPhase::Up, 500.);
            painted = true;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "startup never completed"
        );
    }
    eprintln!(
        "GTK startup: UI built {built:.3}ms, present returned {presented:.3}ms, first canvas feedback {first_canvas:.3?}ms, max event-loop slice {max_pump_ms:.3}ms"
    );
    eprintln!("GTK startup document/brush/all ready: {stages:.3?}ms");
    pump(100);
    assert_eq!(strokes(), 2);
    assert!(white_pixels(&w) > 100_000);
    w.window.destroy();
    pump(20);
}

pub(crate) struct NativeTestApp(pub(crate) adw::Application);
impl std::ops::Deref for NativeTestApp {
    type Target = adw::Application;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for NativeTestApp {
    fn drop(&mut self) {
        // A failed assertion must not leave GPU workers alive while the test
        // process tears down GTK and the Vulkan driver.
        for window in self.0.windows() {
            window.destroy();
        }
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
pub(crate) fn native_test_app(id: &str) -> NativeTestApp {
    adw::init().unwrap();
    let css = crate::stylesheet_provider();
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let app = adw::Application::builder()
        .application_id(id)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    NativeTestApp(app)
}

fn assert_drawer_connected(w: &Workspace) {
    let p = w.drawer.placement().unwrap();
    let c = p.connection().unwrap();
    let connection = find_named(w.surface.upcast_ref(), "drawer-connection").unwrap();
    assert!(connection.is_mapped() && connection.can_target());
    let actual = connection.compute_bounds(&w.surface).unwrap();
    assert!((actual.x() - c.bounds.x).abs() <= 1.0);
    assert!((actual.y() - c.bounds.y).abs() <= 1.0);
    assert!((actual.width() - c.bounds.width).abs() <= 1.0);
    assert!((actual.height() - c.bounds.height).abs() <= 1.0);
    let center = [
        c.bounds.x + c.bounds.width * 0.5,
        c.bounds.y + c.bounds.height * 0.5,
    ];
    assert_eq!(
        w.surface
            .pick(center[0] as f64, center[1] as f64, gtk::PickFlags::DEFAULT),
        Some(connection)
    );
    w.chrome_event(ChromeEvent::Contact {
        position: center,
        canvas: false,
    });
    assert!(
        state(w).customization.drawer.is_some(),
        "the connecting stem is part of the drawer, not an outside click"
    );
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_nested_tool_drawers() {
    let app = native_test_app("art.capycanvas.NestedDrawers");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(1800);
    let original = layer_ui::WorkspaceState::default();
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let output = artifact_dir("../../artifacts/familiar-workspace");
    for (theme, group) in [(Theme::Dark, 5), (Theme::Light, 8)] {
        let mut workspace = original.clone();
        workspace
            .layout
            .move_panel(
                viewport,
                Panel::Toolbar,
                DockTarget::Tab { group, index: None },
            )
            .unwrap();
        let long = workspace
            .layout
            .add_toolbar(Some(group), "Long tools", &vec![ToolbarControl::Color; 180])
            .unwrap();
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(workspace),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::Invoke {
            command: CommandId::Pen,
        });
        w.dispatch(UiAction::DoubleClickPanelHandle { group, viewport });
        enable_individual_column_panels(&w, group);
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::ToggleColumnDrawer {
                group,
                panel: Panel::Toolbar,
            },
        });
        pump(350);
        let column = state(&w)
            .workspace
            .layout
            .collapsed_column_for_group(group)
            .unwrap();
        let parent_name = format!("column-drawer-{column}");
        let parent = || find_named(w.surface.upcast_ref(), &parent_name).unwrap();
        let tiles = state(&w)
            .workspace
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()
            .to_vec();
        let button = |id| {
            named::<gtk::Button>(&parent(), &format!("tile-{id}"))
        };
        click(&button(tiles[0].id));
        pump(80);
        assert!(state(&w).customization.drawer.is_none());
        click(&button(tiles[0].id));
        pump(300);
        assert!(
            state(&w).customization.drawer.is_some(),
            "{:?}",
            state(&w).host_error
        );
        let b = button(tiles[0].id).compute_bounds(&w.surface).unwrap();
        let p = w.drawer.placement().unwrap();
        assert!((p.anchor.x - b.x()).abs() <= 1.);
        assert!((p.anchor.y - b.y()).abs() <= 1.);
        assert!(p.connection().is_some());
        assert_eq!(state(&w).customization.column_drawers.len(), 1);
        assert!(button(tiles[0].id).has_css_class(if group == 5 {
            "drawer-origin-right"
        } else {
            "drawer-origin-left"
        }));
        let point = [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5];
        assert!(
            !w.chrome_event(ChromeEvent::Contact {
                position: point,
                canvas: false
            })
            .handled
        );
        assert!(state(&w).customization.drawer.is_some());
        click(&button(tiles[0].id));
        pump(260);
        assert!(state(&w).customization.drawer.is_none());
        let color = tiles
            .iter()
            .find(|t| t.control == ToolbarControl::Color)
            .unwrap();
        click(&button(color.id));
        pump(300);
        assert_eq!(
            state(&w).customization.drawer.as_ref().unwrap().columns,
            [vec![Panel::Color, Panel::Palettes]]
        );
        capture_reference(&w, &format!("{output}/columns-nested-{theme:?}.png"), 1.);
        w.window.unmaximize();
        pump(350); // Wait for the compositor's restore-size configure first.
        w.window.set_default_size(640, 480);
        pump(350);
        assert_eq!(
            (w.surface.width(), w.surface.height()),
            (640, 480),
            "small viewport must actually be allocated; window minimum {:?}, maximized {}",
            w.window.measure(gtk::Orientation::Horizontal, -1),
            w.window.is_maximized()
        );
        for drawer in w.drawers() {
            let p = drawer
                .placement()
                .expect("drawer remains reachable on a small viewport");
            assert!(p.bounds.x >= WORKSPACE_SPACING - 1. && p.bounds.y >= HEADER_HEIGHT - 1.);
            assert!(
                p.bounds.x + p.bounds.width <= w.surface.width() as f32 - WORKSPACE_SPACING + 1.
            );
            assert!(
                p.bounds.y + p.bounds.height <= w.surface.height() as f32 - WORKSPACE_SPACING + 1.
            );
        }
        capture_reference(
            &w,
            &format!("{output}/columns-nested-small-{theme:?}.png"),
            1.,
        );
        w.window
            .set_default_size(viewport[0] as i32, viewport[1] as i32);
        pump(350);
        w.chrome_event(ChromeEvent::Contact {
            position: [viewport[0] * 0.5, viewport[1] - 50.],
            canvas: true,
        });
        pump(260);
        assert!(state(&w).customization.drawer.is_none());
        assert_eq!(state(&w).customization.column_drawers.len(), 1);

        // The same projection supports a long toolbar. Move a still-visible
        // source by scrolling and check that the child follows in native pixels.
        w.dispatch(UiAction::SelectPanelTab { group, panel: long });
        pump(300);
        let id = state(&w).workspace.layout.panel(long).unwrap().tiles()[65].id;
        let origin = button(id);
        let mut ancestor = origin.parent().unwrap();
        while !ancestor.is::<gtk::ScrolledWindow>() {
            ancestor = ancestor.parent().unwrap();
        }
        let scroller = ancestor.downcast::<gtk::ScrolledWindow>().unwrap();
        scroller.vadjustment().set_value(24.);
        pump(100);
        click(&origin);
        pump(300);
        assert!(
            state(&w).customization.drawer.is_some(),
            "origin {:?}, scroll {:?}, viewport {:?}; {:?}",
            origin.compute_bounds(&w.surface),
            scroller.vadjustment().value(),
            scroller.compute_bounds(&w.surface),
            state(&w).host_error
        );
        let before = w.drawer.placement().unwrap();
        scroller.vadjustment().set_value(48.);
        pump(100);
        let after = w.drawer.placement().unwrap();
        let b = origin.compute_bounds(&w.surface).unwrap();
        assert!((after.anchor.y - b.y()).abs() <= 1., "{after:?} vs {b:?}");
        assert!((after.anchor.y - before.anchor.y + 24.).abs() <= 1.);
        let child = find_named(w.surface.upcast_ref(), "tool-drawer").unwrap();
        let picked = w
            .surface
            .pick(
                (after.bounds.x + 10.) as f64,
                (after.bounds.y + 40.) as f64,
                gtk::PickFlags::DEFAULT,
            )
            .unwrap();
        assert!(
            picked == child || picked.is_ancestor(&child),
            "parent must not cover child"
        );
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::CloseExpanded,
        });
        scroller.vadjustment().set_value(0.);
        pump(260);
        let first = state(&w).workspace.layout.panel(long).unwrap().tiles()[0].id;
        click(&button(first));
        pump(260);
        assert!(w.drawer.placement().is_some());
        scroller.vadjustment().set_value(48.);
        pump(120);
        assert!(
            w.drawer.placement().is_none(),
            "clipped origin must not leave an invisible hit region"
        );
        assert!(
            !find_named(w.surface.upcast_ref(), "tool-drawer")
                .unwrap()
                .is_mapped()
        );
        scroller.vadjustment().set_value(0.);
        pump(120);
        assert!(w.drawer.placement().is_some());
        // Switching the parent tab removes its obsolete tool drawer.
        w.dispatch(UiAction::SelectPanelTab {
            group,
            panel: Panel::Toolbar,
        });
        pump(300);
        assert!(state(&w).customization.drawer.is_none());
        assert_eq!(state(&w).customization.column_drawers.len(), 1);
    }
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_collapsed_columns() {
    let app = native_test_app("art.capycanvas.CollapsedColumns");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(1800);
    let original_workspace = layer_ui::WorkspaceState::default();
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let output = artifact_dir("../../artifacts/familiar-workspace");
    let press = |name: &str| {
        let button = find_named(w.surface.upcast_ref(), name)
            .unwrap_or_else(|| panic!("Missing {name}"))
            .downcast::<gtk::Button>()
            .unwrap();
        assert!(button.is_mapped(), "{name} must be visible");
        click(&button);
        pump(280);
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(original_workspace.clone()),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::DoubleClickPanelHandle { group: 5, viewport });
        let menu = ui_session(&w)
            .context_menu(ContextTarget::Group { group: 8 })
            .unwrap();
        let collapse = menu
            .sections
            .iter()
            .flatten()
            .find(|i| i.label == "Collapse column")
            .unwrap()
            .action
            .clone()
            .unwrap();
        w.dispatch(collapse);
        pump(250);
        assert_eq!(state(&w).workspace.layout.collapsed.len(), 2);
        for col in w.resolved().collapsed {
            let widget = find_named(
                w.surface.upcast_ref(),
                &format!("collapsed-column-{}", col.id),
            )
            .unwrap();
            let b = widget.compute_bounds(&w.surface).unwrap();
            assert!((b.width() - TILE_SIZE).abs() <= 1.);
            assert!((b.y() - col.bounds.y).abs() <= 1.);
            assert!((b.height() - col.bounds.height).abs() <= 1.);
            for icon in col.groups.iter().flat_map(|g| &g.icons) {
                let button = find_named(
                    w.surface.upcast_ref(),
                    &format!("column-icon-{:?}", icon.panel),
                )
                .unwrap();
                let b = button.compute_bounds(&w.surface).unwrap();
                assert!(
                    (b.y() - icon.bounds.y).abs() <= 1.,
                    "{:?}: {b:?} vs {:?}",
                    icon.panel,
                    icon.bounds
                );
                assert_eq!(b.width(), TILE_SIZE);
                assert_eq!(b.height(), TILE_SIZE);
            }
        }
        capture_reference(
            &w,
            &format!("{output}/columns-collapsed-{theme:?}.png"),
            1.0,
        );
        enable_individual_column_panels(&w, 5);
        enable_individual_column_panels(&w, 8);
        press("column-icon-Brushes");
        press("column-icon-Layers");
        assert_eq!(state(&w).customization.column_drawers.len(), 2);
        w.chrome_event(ChromeEvent::Contact {
            position: [viewport[0] * 0.5, viewport[1] * 0.8],
            canvas: true,
        });
        pump(50);
        assert_eq!(state(&w).customization.column_drawers.len(), 2);
        let width = find_named(w.surface.upcast_ref(), "column-drawer-8")
            .unwrap()
            .width();
        press("column-drawer-tab-Properties");
        assert_eq!(
            find_named(w.surface.upcast_ref(), "column-drawer-8")
                .unwrap()
                .width(),
            width
        );
        assert_eq!(
            w.resolved()
                .collapsed
                .iter()
                .flat_map(|c| &c.groups)
                .find(|g| g.group == 8)
                .map(|g| g.active),
            Some(Panel::Properties)
        );
        press("column-drawer-tab-Adjustments");
        pump(500);
        assert_eq!(
            find_named(w.surface.upcast_ref(), "column-drawer-8")
                .unwrap()
                .width(),
            width
        );
        capture_reference(&w, &format!("{output}/columns-drawers-{theme:?}.png"), 1.0);
        press("column-icon-Layers");
        assert_eq!(state(&w).customization.column_drawers.len(), 2);
        press("column-icon-Layers");
        assert_eq!(state(&w).customization.column_drawers.len(), 1);
        press("column-icon-Sizes");
        assert_eq!(state(&w).customization.column_drawers.len(), 1);
        let layout = w.resolved();
        let grip = layout.collapsed.iter().find(|c| c.id == 4).unwrap().grip;
        let point = [grip.x + grip.width * 0.5, grip.y + grip.height * 0.5];
        assert!(matches!(
            w.drag_target_at(point),
            Some(DragTarget::Dock(DockItem::Column { column: 4 }))
        ));
        let right = layout.collapsed.iter().find(|c| c.id == 8).unwrap().bounds;
        let destination = [right.x - 2., right.y + 200.];
        w.workspace_drag_input(ContactPhase::Down, point, None);
        w.workspace_drag_input(ContactPhase::Move, destination, None);
        pump(50); // Workspace geometry and hints publish on the display clock.
        assert!(w.drop_hint.borrow().is_some());
        w.workspace_drag_input(ContactPhase::Up, destination, None);
        pump(250);
        assert!(state(&w).workspace.layout.floating.is_empty());
        assert_eq!(state(&w).workspace.layout.collapsed.len(), 2);
        let r = w.resolved();
        assert!(r.collapsed.iter().find(|c| c.id == 4).unwrap().bounds.x > viewport[0] * 0.5);
        for group in [4, 8] {
            w.dispatch(UiAction::Customize { action: CustomizationAction::SetColumnCollapsed { group, collapsed: false } });
            pump(150);
        }
        assert!(state(&w).workspace.layout.collapsed.is_empty());
        assert!(state(&w).customization.column_drawers.is_empty());
    }
    // Overflow is a native scroll area; the shared hit targets must follow it
    // and retain its offset when a membership change rebuilds the widgets.
    let mut workspace = original_workspace;
    for i in 0..28 {
        workspace
            .layout
            .add_toolbar(Some(8), &format!("Test toolbar {i}"), &[])
            .unwrap();
    }
    workspace
        .layout
        .set_column_collapsed(8, true, viewport)
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(250);
    let scrolling = || {
        named::<gtk::ScrolledWindow>(w.surface.upcast_ref(), "column-scroll-8")
    };
    scrolling().vadjustment().set_value(280.);
    pump(120);
    assert_eq!(state(&w).workspace.layout.column_scroll, [(8, 280.)]);
    let verify_scroll = || {
        for c in &w.resolved().collapsed {
            for icon in c.groups.iter().flat_map(|g| &g.icons) {
                if icon.bounds.y + icon.bounds.height <= c.content.y
                    || icon.bounds.y >= c.content.y + c.content.height
                {
                    continue;
                }
                let button = find_named(
                    w.surface.upcast_ref(),
                    &format!("column-icon-{:?}", icon.panel),
                )
                .unwrap();
                let b = button.compute_bounds(&w.surface).unwrap();
                assert!(
                    (b.y() - icon.bounds.y).abs() <= 1.,
                    "Scrolled {:?}: {b:?} vs {:?}",
                    icon.panel,
                    icon.bounds
                );
            }
        }
    };
    verify_scroll();
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetPanelVisible {
            panel: Panel::Properties,
            visible: false,
        },
    });
    pump(250);
    assert_eq!(scrolling().vadjustment().value(), 280.);
    verify_scroll();
    w.window.destroy();
    pump(50);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_tool_drawers() {
    let app = native_test_app("art.capycanvas.ToolDrawers");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(800);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut workspace = layer_ui::WorkspaceState::default();
    let old = workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .to_vec();
    for tile in old {
        workspace
            .layout
            .remove_tool(Panel::Toolbar, tile.id)
            .unwrap();
    }
    let controls = [
        ToolbarControl::Command {
            command: CommandId::Pen,
        },
        ToolbarControl::Command {
            command: CommandId::Pencil,
        },
        ToolbarControl::Color,
    ]
    .into_iter()
    .chain(
        Panel::ALL
            .into_iter()
            .filter(|p| p.kind() == PanelKind::Content)
            .map(|panel| ToolbarControl::Panel { panel }),
    )
    .collect::<Vec<_>>();
    workspace
        .layout
        .insert_tools(Panel::Toolbar, None, &controls)
        .unwrap();
    let ids = workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .iter()
        .map(|t| t.id)
        .collect::<Vec<_>>();
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::Toolbar,
            DockTarget::Edge {
                edge: Edge::Left,
                outer: true,
            },
        )
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(200);
    let output = artifact_dir("../../artifacts/familiar-workspace");
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for id in &ids {
            let button = named::<gtk::Button>(w.surface.upcast_ref(), &format!("tile-{id}"));
            click(&button);
            if state(&w).customization.drawer.is_none() {
                click(&button);
            }
            pump(300);
            assert!(state(&w).customization.drawer.is_some(), "tile {id}");
            assert_drawer_connected(&w);
            let drawer = find_named(w.surface.upcast_ref(), "tool-drawer").unwrap();
            assert!(drawer.is_mapped());
            let b = drawer.compute_bounds(&w.surface).unwrap();
            assert!(b.width() > 100.0 && b.height() > 30.0, "tile {id}: {b:?}");
            assert!(b.x() >= 0.0 && b.y() >= HEADER_HEIGHT);
            assert!(b.x() + b.width() <= viewport[0] && b.y() + b.height() <= viewport[1]);
            if *id == ids[0] {
                let size = named::<crate::number_control::NumberControl>(&drawer, "tool-setting-size");
                edit_number(&size, "12*3");
                assert_eq!(state(&w).brush.diameter, 36.0);
                assert_eq!(w.size_number.value(), 36.0);
            }
            if controls[ids.iter().position(|t| t == id).unwrap()]
                == (ToolbarControl::Panel {
                    panel: Panel::Stats,
                })
            {
                w.dispatch(UiAction::SetLayerOpacity {
                    id: None,
                    opacity: if theme == Theme::Dark { 0.9 } else { 1.0 },
                });
                pump(250);
                let telemetry = ui_session(&w).renderer_stats();
                assert_ne!(
                    telemetry
                        .rows
                        .iter()
                        .find(|r| r.label == "GPU · ms")
                        .unwrap()
                        .value,
                    "Unavailable"
                );
                assert_ne!(
                    telemetry
                        .rows
                        .iter()
                        .find(|r| r.label == "Frames")
                        .unwrap()
                        .value,
                    "0"
                );
            }
            // The original dock still owns its own live panel body.
            for (panel, widget) in &w.panels {
                if state(&w).workspace.layout.panel_group(*panel).is_some() {
                    assert!(widget.parent().is_some());
                }
            }
            capture_reference(&w, &format!("{output}/drawer-{id}-{theme:?}.png"), 1.0);
            let origin = ["drawer-origin-right", "drawer-open"];
            assert!(origin.iter().all(|class| button.has_css_class(class)), "tile {id}");
            click(&button);
            assert!(state(&w).customization.drawer.is_none());
            assert!(!w.drawer.is_closed(), "tile {id}: drawer is still closing");
            assert!(
                origin.iter().all(|class| button.has_css_class(class)),
                "tile {id}: opener stays joined until the drawer has closed"
            );
            pump(240);
            assert!(find_named(w.surface.upcast_ref(), "tool-drawer").is_none());
            assert!(!origin.iter().any(|class| button.has_css_class(class)), "tile {id}");
        }
    }
    // Two live filter projections share one GPU producer and the same textures.
    w.dispatch(UiAction::SelectPanelTab {
        group: 8,
        panel: Panel::Adjustments,
    });
    pump(1000);
    let panel_tile = |panel| {
        let index = controls
            .iter()
            .position(|c| *c == ToolbarControl::Panel { panel })
            .unwrap();
        named::<gtk::Button>(w.surface.upcast_ref(), &format!("tile-{}", ids[index]))
    };
    let filter_button = panel_tile(Panel::Adjustments);
    click(&filter_button);
    pump(1200);
    let filter_texture = |root: &gtk::Widget| {
        find_css(root, "filter-row")
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Picture>()
            .unwrap()
            .paintable()
            .unwrap()
    };
    assert_eq!(
        filter_texture(w.effects.adjustments.upcast_ref()),
        filter_texture(w.drawer.effects().unwrap().adjustments.upcast_ref())
    );
    for root in [
        w.effects.adjustments.clone(),
        w.drawer.effects().unwrap().adjustments.clone(),
    ] {
        let scroll = find_css(root.upcast_ref(), "filter-picker-scroll").unwrap();
        let bounds = scroll.compute_bounds(&root).unwrap();
        assert_eq!(bounds.x(), 0.0);
        assert_eq!(
            bounds.width(),
            root.width() as f32,
            "Filters scrollbar reaches the panel edge in docks and drawers"
        );
        let header = find_css(root.upcast_ref(), "filter-picker-header").unwrap();
        let (content, inset) = if header.is_visible() {
            (header, 6.0)
        } else {
            (find_css(root.upcast_ref(), "filter-picker-body").unwrap(), 8.0)
        };
        assert_eq!(
            content.compute_bounds(&root).unwrap().x(),
            inset,
            "moving the scrollbar preserves content padding"
        );
    }
    assert!(w.effects.preview_requests() > 0);
    click(&filter_button);
    pump(500);
    click(&filter_button);
    pump(800);
    let requests = w.effects.preview_requests();
    pump(800);
    assert_eq!(
        w.effects.preview_requests(),
        requests,
        "reopened filter previews settle without re-requesting"
    );
    click(&filter_button);
    pump(250);
    let layers = panel_tile(Panel::Layers);
    let requests = w.layer_panel.preview_requests();
    click(&layers);
    pump(500);
    assert_eq!(
        w.layer_panel.preview_requests(),
        requests,
        "unchanged layers reuse cached thumbnails"
    );
    let layer_view = w.drawer.layers().unwrap();
    click(
        &layer_view
            .footer
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap(),
    );
    pump(80);
    let menu = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .find(|p| p.is_visible())
        .unwrap();
    assert_eq!(menu.parent().as_ref(), Some(layer_view.root.upcast_ref()));
    assert!(
        menu.is_mapped(),
        "the menu belongs to its drawer projection, not the hidden dock"
    );
    menu.popdown();
    pump(50);
    click(&layers);
    pump(250);
    for edge in [Edge::Top, Edge::Bottom, Edge::Right] {
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Toolbar,
            target: DockTarget::Edge { edge, outer: false },
            viewport,
        });
        pump(150);
        let button = named::<gtk::Button>(w.surface.upcast_ref(), &format!("tile-{}", ids[2]));
        click(&button);
        pump(300);
        assert!(state(&w).customization.drawer.is_some());
        assert_drawer_connected(&w);
        capture_reference(&w, &format!("{output}/drawer-color-{edge:?}.png"), 1.0);
        let dismissed = w.chrome_event(ChromeEvent::Contact {
            position: [1100.0, 700.0],
            canvas: true,
        });
        assert!(dismissed.handled);
        assert!(state(&w).customization.drawer.is_none());
        pump(250);
    }
    w.window.close();
    pump(50);
}

fn native_pen_path(w: &Rc<Workspace>, points: &[[f32; 2]]) {
    // The native host rejects an entire contact begun before brush readiness.
    // Correctness fixtures must wait for that gate after selecting a brush or
    // changing its selection dependencies, rather than assume a startup delay.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if w.gpu.borrow().as_ref().is_some_and(|g| {
            let engine = g.session.engine();
            engine.backend().paint_ready(
                engine.document(), engine.brush(), engine.transform_preview().is_some(),
            )
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "native contact startup: {}", w.status.text());
        pump(5);
    }
    let camera = state(w).camera;
    for (i, p) in points.iter().enumerate() {
        let now = glib::monotonic_time() as u64 * 1000;
        let phase = match i {
            0 => PenPhase::Down,
            _ if i + 1 == points.len() => PenPhase::Up,
            _ => PenPhase::Move,
        };
        let event = pen_event(&camera, *p, phase, now);
        w.input.send(
            w,
            PenEvent {
                device_id: 92,
                timestamp_ns: now,
                ..event
            },
        );
        pump(20);
    }
    pump(180);
    assert!(!w.status.is_visible(), "{}", w.status.text());
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_connected_tools() {
    let app = native_test_app("art.capycanvas.ConnectedTools");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let workspace = tool_settings_workspace(&w, &[CommandId::AutoSelect, CommandId::Fill], false, false);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    w.dispatch(UiAction::SelectBrush {
        id: layer_core::DefaultBrushPreset::GPen as u32,
    });
    w.dispatch(UiAction::SetBrushSize { value: 18. });
    w.dispatch(UiAction::SetColor {
        rgba: [0.15, 0.15, 0.15, 1.],
    });
    native_pen_path(
        &w,
        &[
            [1014., 470.],
            [1290., 470.],
            [1290., 1060.],
            [750., 1060.],
            [750., 470.],
            [990., 470.],
        ],
    );
    assert_eq!(
        ui_session(&w)
            .engine()
            .metrics()
            .committed_strokes,
        1,
        "gap closing requires a committed ink boundary"
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::AutoSelect,
    });
    w.dispatch(UiAction::Layer {
        action: LayerAction::ReferenceSelection,
    });
    let reference_source = UiAction::Invoke { command: CommandId::SelectionReference };
    w.dispatch(reference_source);
    pump(100);
    let tolerance = named::<crate::number_control::NumberControl>(&w.panel_widget(Panel::ToolSettings), "tool-setting-tolerance");
    edit_number(&tolerance, "15");
    native_pen_path(&w, &[[1000., 750.], [1000., 750.]]);
    pump(350);
    let selected_bounds = || {
        let gpu = w.gpu.borrow();
        let selection = gpu
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .working.selection
            .as_ref()
            .unwrap();
        let layer_core::SelectionShape::Pixels(pixels) = &selection.shape else {
            panic!("raster selection")
        };
        pixels.bounds()
    };
    assert_eq!(
        selected_bounds(),
        [0, 0, 2048, 1536],
        "the open line must actually leak before gap closing"
    );
    let setting = |id: &str| {
        named::<crate::number_control::NumberControl>(&w.panel_widget(Panel::ToolSettings), &format!("tool-setting-{id}"))
    };
    edit_number(&setting("gap_closing"), "12");
    edit_number(&setting("smoothing"), "100");
    native_pen_path(&w, &[[1000., 750.], [1000., 750.]]);
    pump(350);
    let selection = ui_session(&w)
        .engine()
        .document()
        .working.selection
        .clone()
        .expect("connected selection");
    let layer_core::SelectionShape::Pixels(pixels) = &selection.shape else {
        panic!("raster selection")
    };
    let [x0, y0, x1, y1] = pixels.bounds();
    assert!(
        x0 > 750 && y0 > 470 && x1 < 1290 && y1 < 1060,
        "{:?}",
        pixels.bounds()
    );
    let dir = artifact_dir("../../artifacts/familiar-workspace");
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(120);
        capture_reference(&w, &format!("{dir}/region-selection-{theme:?}.png"), 1.);
    }
    w.dispatch(UiAction::Invoke {
        command: CommandId::AddLayer,
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::LowerLayer,
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.1, 0.6, 0.8, 1.],
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::Fill,
    });
    let reference_source = state(&w).tool_set.subtools[2].action.clone();
    w.dispatch(reference_source);
    pump(100);
    edit_number(&setting("expansion"), "2");
    let fill_raster = || {
        let gpu = w.gpu.borrow();
        let doc = gpu.as_ref().unwrap().session.engine().document();
        active_raster(doc).clone()
    };
    let before_fill = fill_raster();
    assert!(before_fill.is_empty());
    native_pen_path(&w, &[[1000., 750.], [1000., 750.]]);
    pump(250);
    let after_fill = fill_raster();
    assert!(
        !after_fill.try_data().expect("published fill index").unwrap().tiles.is_empty(),
        "fill commits raster pixels"
    );
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(120);
        capture_reference(&w, &format!("{dir}/region-fill-{theme:?}.png"), 1.);
    }
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(100);
    assert_eq!(fill_raster(), before_fill, "undo restores the exact empty raster");
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    pump(100);
    assert_eq!(fill_raster(), after_fill, "redo restores the committed fill raster");
    w.dispatch(UiAction::SetColor { rgba: [1.; 4] });
    w.dispatch(UiAction::Layer {
        action: LayerAction::Tool {
            tool: LayerCanvasTool::PickLayer,
        },
    });
    native_pen_path(&w, &[[1000., 750.], [1000., 750.]]);
    assert!(
        state(&w).colors.foreground.rgba[0] < 0.2 && state(&w).colors.foreground.rgba[2] > 0.75,
        "{:?}",
        state(&w).colors.foreground
    );
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_ruler_tools() {
    let app = native_test_app("art.capycanvas.RulerTools");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let workspace = tool_settings_workspace(&w, &[CommandId::Ruler], false, true);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(100);
    w.dispatch(UiAction::Invoke {
        command: CommandId::FitCanvas,
    });
    let ruler_count = || {
        ui_session(&w)
            .engine()
            .document().rulers().count()
    };
    let dir = artifact_dir("../../artifacts/familiar-workspace");
    for index in 0..3 {
        w.dispatch(UiAction::Invoke {
            command: CommandId::Ruler,
        });
        pump(50);
        let button = w.tool_set.group_buttons.borrow()[index].clone();
        click(&button);
        pump(50);
        let path = match index {
            0 => [[450., 400.], [1550., 400.]],
            1 => [[500., 720.], [700., 880.]],
            _ => [[1470., 1120.], [1470., 1120.]],
        };
        native_pen_path(&w, &path);
        assert_eq!(ruler_count(), index + 1);
        let toggle = named::<gtk::CheckButton>(&w.panel_widget(Panel::ToolSettings), "tool-action-SnapRulers");
        assert!(toggle.is_active());
        w.dispatch(UiAction::SelectBrush {
            id: layer_core::DefaultBrushPreset::GPen as u32,
        });
        w.dispatch(UiAction::SetBrushSize { value: 18. });
        w.dispatch(UiAction::SetColor {
            rgba: [
                [0.15, 0.35, 0.85, 1.],
                [0.1, 0.6, 0.3, 1.],
                [0.85, 0.3, 0.12, 1.],
            ][index],
        });
        if index < 2 {
            let points: Vec<_> = (0..40)
                .map(|i| {
                    let x = 800. + i as f32 * 12.;
                    [
                        x,
                        if index == 0 {
                            410. + 20. * (i as f32 * 0.5).sin()
                        } else {
                            800. + i as f32 * 6. + 15. * (i as f32 * 0.7).sin()
                        },
                    ]
                })
                .collect();
            native_pen_path(&w, &points);
        } else {
            for dx in [-360., -180., 0., 180., 360.] {
                native_pen_path(
                    &w,
                    &[
                        [1470., 1120.],
                        [1470. + dx * 0.3, 1050.],
                        [1470. + dx * 0.6 + 15., 980.],
                        [1470. + dx, 890.],
                    ],
                );
            }
        }
    }
    // Verify the snapped path really reaches GPU pixels, independently of guides.
    w.dispatch(UiAction::Layer {
        action: LayerAction::Tool {
            tool: LayerCanvasTool::PickLayer,
        },
    });
    native_pen_path(&w, &[[1000., 400.], [1000., 400.]]);
    pump(100);
    let color = state(&w).colors.foreground.rgba;
    assert!(
        color[2] > 0.7 && color[0] < 0.25,
        "snapped GPU ink: {color:?}"
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::Ruler,
    });
    pump(50);
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(100);
        capture_reference(&w, &format!("{dir}/rulers-{theme:?}.png"), 1.);
    }
    let show = named::<gtk::CheckButton>(&w.panel_widget(Panel::ToolSettings), "tool-action-ShowRulers");
    show.set_active(false);
    pump(100);
    let snap = named::<gtk::CheckButton>(&w.panel_widget(Panel::ToolSettings), "tool-action-SnapRulers");
    assert!(!snap.is_sensitive());
    capture_reference(&w, &format!("{dir}/rulers-hidden.png"), 1.);
    show.set_active(true);
    pump(100);
    assert!(snap.is_sensitive());
    // Select and delete a center; undo restores its guide without changing ink.
    native_pen_path(&w, &[[1470., 1120.], [1470., 1120.]]);
    let delete = named::<gtk::Button>(&w.panel_widget(Panel::ToolSettings), "tool-action-DeleteRuler");
    click(&delete);
    pump(100);
    assert_eq!(ruler_count(), 2);
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(100);
    assert_eq!(ruler_count(), 3);
    assert!(!w.status.is_visible(), "{}", w.status.text());
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_operation_tool() {
    let app = native_test_app("art.capycanvas.OperationTool");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let workspace = tool_settings_workspace(&w, &[], true, true);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(100);
    let divider = w
        .resolved()
        .dividers
        .into_iter()
        .find(|d| d.axis == Axis::Vertical && d.bounds.x < 100.)
        .unwrap();
    drag_divider(&w, divider.id, [100., 220.]);
    w.dispatch(UiAction::Invoke {
        command: CommandId::FitCanvas,
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.12, 0.38, 0.72, 1.],
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::Lasso,
    });
    native_pen_path(
        &w,
        &[
            [650., 500.],
            [1350., 500.],
            [1350., 1000.],
            [650., 1000.],
            [650., 500.],
        ],
    );
    w.dispatch(UiAction::Layer {
        action: LayerAction::FillSelection,
    });
    pump(200);
    let document = || {
        ui_session(&w)
            .engine()
            .document()
            .clone()
    };
    let original = document();
    w.dispatch(UiAction::Invoke {
        command: CommandId::Move,
    });
    pump(100);
    assert_eq!(w.tool_set.group_buttons.borrow().len(), 2);
    let transform = w.tool_set.group_buttons.borrow()[1].clone();
    click(&transform);
    assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::Transform);
    // Exercise the real canvas input/controller: moving inside the box, then
    // its lower-right handle. The opposite corner must not jump on resize.
    native_pen_path(&w, &[[1000., 750.], [1100., 800.]]);
    let value = |id: &str| {
        state(&w)
            .tool_settings
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .value
    };
    assert!((value("transform_x") - 100.).abs() < 0.1);
    assert!((value("transform_y") - 50.).abs() < 0.1);
    native_pen_path(&w, &[[1450., 1050.], [1590., 1150.]]);
    assert!((value("transform_width") - 1.2).abs() < 0.01);
    assert!((value("transform_height") - 1.2).abs() < 0.01);
    let number = |id: &str| {
        named::<crate::number_control::NumberControl>(&w.panel_widget(Panel::ToolSettings), &format!("tool-setting-{id}"))
    };
    edit_number(&number("transform_angle"), "360/12");
    pump(150);
    assert!((value("transform_angle") - std::f32::consts::PI / 6.).abs() < 0.001);
    let uniform: gtk::ToggleButton = find_named(
        &w.panel_widget(Panel::ToolSettings),
        "tool-action-TransformUniform",
    )
    .unwrap()
    .downcast()
    .unwrap();
    uniform.set_active(true);
    edit_number(&number("transform_width"), "150");
    pump(100);
    assert!((value("transform_height") - 1.5).abs() < 0.01);
    assert_eq!(
        document().revision,
        original.revision,
        "preview does not edit history"
    );
    let dir = artifact_dir("../../artifacts/familiar-workspace");
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(150);
        number("transform_x")
            .ancestor(gtk::ScrolledWindow::static_type())
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap()
            .vadjustment()
            .set_value(0.);
        pump(50);
        capture_reference(&w, &format!("{dir}/operation-{theme:?}.png"), 1.);
        let panel = w.panel_widget(Panel::ToolSettings);
        let before_width = panel.width();
        let edge = panel.compute_bounds(&w.surface).unwrap().x() + before_width as f32;
        let divider = w
            .resolved()
            .dividers
            .into_iter()
            .filter(|d| d.band && d.axis == Axis::Horizontal)
            .min_by(|a, b| (a.bounds.x - edge).abs().total_cmp(&(b.bounds.x - edge).abs()))
            .unwrap();
        let minimum = layer_ui::TOOL_SETTINGS_MIN_WIDTH as i32;
        drag_divider(
            &w,
            divider.id,
            [divider.parent.x + minimum as f32 + WORKSPACE_SPACING * 0.5, 0.],
        );
        pump(120);
        assert_eq!(panel.width(), minimum);
        assert!(
            w.tool_settings
                .root
                .measure(gtk::Orientation::Horizontal, -1)
                .0
                <= minimum,
            "tool settings exceed their six-tile minimum"
        );
        capture_reference(&w, &format!("{dir}/operation-narrow-{theme:?}.png"), 1.);
        drag_divider(
            &w,
            divider.id,
            [divider.bounds.x + divider.bounds.width * 0.5, 0.],
        );
        pump(120);
    }
    let cancel: gtk::Button = find_named(
        &w.panel_widget(Panel::ToolSettings),
        "tool-action-CancelTransform",
    )
    .unwrap()
    .downcast()
    .unwrap();
    click(&cancel);
    assert_eq!(document().artwork, original.artwork);
    assert_eq!(document().working.selection, original.working.selection);
    assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::Move);
    let transform = w.tool_set.group_buttons.borrow()[1].clone();
    click(&transform);
    edit_number(&number("transform_x"), "240");
    let apply: gtk::Button = find_named(
        &w.panel_widget(Panel::ToolSettings),
        "tool-action-ApplyTransform",
    )
    .unwrap()
    .downcast()
    .unwrap();
    click(&apply);
    pump(150);
    assert_ne!(
        document().target_raster(original.working.target.unwrap()).unwrap(),
        active_raster(&original),
        "applying a transform publishes a new raster root"
    );
    assert_ne!(document().working.selection, original.working.selection);
    // Sample real GPU pixels after Apply, then undo; the displaced left edge
    // becomes paper and returns to blue. This is not only a model assertion.
    w.dispatch(UiAction::Layer {
        action: LayerAction::Deselect,
    });
    let sample = |point: [f32; 2]| {
        w.dispatch(UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::PickVisible,
            },
        });
        native_pen_path(&w, &[point, point]);
        until(
            || !state(&w).layer_tools.tool.picks_color(),
            "visible color sample",
        );
        state(&w).colors.foreground.rgba
    };
    assert!(
        sample([700., 750.])[0] > 0.9,
        "old location should be paper"
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    }); // deselect
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    }); // transform + selection
    pump(200);
    assert_eq!(document().artwork, original.artwork);
    assert_eq!(document().working.selection, original.working.selection);
    let c = sample([700., 750.]);
    assert!(c[2] > 0.6 && c[0] < 0.2, "restored ink: {c:?}");
    assert!(!w.status.is_visible(), "{}", w.status.text());
    w.window.destroy();
    pump(50);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_figure_tools() {
    let app = native_test_app("art.capycanvas.FigureTools");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let workspace = tool_settings_workspace(&w, &[CommandId::Figure], false, true);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(100);
    w.dispatch(UiAction::Invoke {
        command: CommandId::FitCanvas,
    });
    w.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Select {
            slot: layer_ui::ColorSlot::Background,
        },
    });
    w.dispatch(UiAction::SetColor {
        rgba: [1., 0.66, 0.15, 1.],
    });
    w.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Select {
            slot: layer_ui::ColorSlot::Foreground,
        },
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.12, 0.30, 0.75, 1.],
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::Figure,
    });
    pump(100);
    assert_eq!(w.tool_set.group_buttons.borrow().len(), 3);
    let width = named::<crate::number_control::NumberControl>(&w.panel_widget(Panel::ToolSettings), "tool-setting-size");
    edit_number(&width, "18");
    assert_eq!(state(&w).brush.diameter, 18.);
    let opacity = named::<crate::number_control::NumberControl>(&w.panel_widget(Panel::ToolSettings), "tool-setting-opacity");
    edit_number(&opacity, "85");
    assert!((state(&w).brush.opacity - 0.85).abs() < 0.001);
    let send = |phase, p: [f32; 2]| {
        let e = pen_event(&state(&w).camera, p, phase, 0);
        w.cursor_input(Some(e));
        w.input.send(&w, e);
        pump(30);
    };
    let dir = artifact_dir("../../artifacts/familiar-workspace");
    for row in 0..3 {
        let group = w.tool_set.group_buttons.borrow()[row].clone();
        click(&group);
        pump(50);
        for col in 0..if row == 0 { 1 } else { 3 } {
            let subtool = w.tool_set.buttons.borrow()[col].1.clone();
            click(&subtool);
            pump(40);
            let start = [440. + col as f32 * 390., 300. + row as f32 * 400.];
            let end = [start[0] + 280., start[1] + 220.];
            send(PenPhase::Down, start);
            send(PenPhase::Move, end);
            if row == 2 && col == 2 {
                pump(100);
                capture_reference(&w, &format!("{dir}/figure-guide.png"), 1.);
            }
            send(PenPhase::Up, end);
            pump(100);
            assert!(!w.status.is_visible(), "{}", w.status.text());
        }
    }
    let controllers = w.window.observe_controllers();
    let keys = (0..controllers.n_items())
        .find_map(|i| {
            controllers
                .item(i)
                .and_downcast::<gtk::EventControllerKey>()
                .filter(|controller| controller.name().as_deref() == Some("workspace-shortcuts"))
        })
        .unwrap();
    for index in 1..3 {
        let button = w.tool_set.group_buttons.borrow()[index].clone();
        click(&button);
        pump(40);
        let start = [830. + (index - 1) as f32 * 390., 300.];
        let end = [start[0] + 200., start[1] + 80.];
        let before = active_raster(ui_session(&w).engine().document()).clone();
        send(PenPhase::Down, start);
        send(PenPhase::Move, end);
        keys.emit_by_name::<bool>(
            "key-pressed",
            &[&gdk::Key::Shift_L, &0u32, &gdk::ModifierType::empty()],
        );
        // Completed operations are baked and discarded. Check the constrained
        // guide before release, then require an actual committed raster edit.
        let mut guide = Vec::new();
        ui_session(&w).append_layer_overlay(&mut guide);
        assert!(!guide.is_empty());
        let gpu = w.gpu.borrow();
        let camera = &gpu.as_ref().unwrap().session.state().camera;
        let inverse = camera.input_transform();
        let scale = w.area.scale_factor() as f32;
        let points: Vec<_> = guide.iter().flat_map(|segment| [segment.from, segment.to])
            .map(|p| inverse.map(Point { x: p[0]*scale, y: p[1]*scale })).collect();
        drop(gpu);
        if index == 1 {
            let min = points.iter().fold([f32::INFINITY; 2], |a, p| [a[0].min(p.x), a[1].min(p.y)]);
            let max = points.iter().fold([f32::NEG_INFINITY; 2], |a, p| [a[0].max(p.x), a[1].max(p.y)]);
            assert!(((max[0]-min[0])-(max[1]-min[1])).abs() < 0.001,
                "native Shift constrains rectangle proportions: {min:?}..{max:?}");
        } else {
            // Ellipse guide tessellation need not include cardinal vertices.
            // Every point must lie on the expected 200px-diameter circle.
            for p in points {
                let radius = (p.x-start[0]-100.).hypot(p.y-start[1]-100.);
                assert!((radius-100.).abs() < 0.001, "native Shift constrains ellipse proportions: {radius}");
            }
        }
        send(PenPhase::Up, end);
        pump(100);
        keys.emit_by_name::<()>(
            "key-released",
            &[&gdk::Key::Shift_L, &0u32, &gdk::ModifierType::SHIFT_MASK],
        );
        new_photo::ready(&w);
        let after = active_raster(ui_session(&w).engine().document()).clone();
        assert_ne!(after, before);
        assert!(after.host_backed());
        assert!(!w.status.is_visible(), "{}", w.status.text());
    }
    w.cursor_input(None);
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(150);
        capture_reference(&w, &format!("{dir}/figures-{theme:?}.png"), 1.);
    }
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(100);
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    pump(100);
    assert!(!w.status.is_visible(), "{}", w.status.text());
    // Real asynchronous GPU sampling verifies ink, not only command state.
    w.dispatch(UiAction::Layer {
        action: LayerAction::Tool {
            tool: LayerCanvasTool::PickLayer,
        },
    });
    send(PenPhase::Down, [950., 800.]);
    send(PenPhase::Up, [950., 800.]);
    pump(150);
    let color = state(&w).colors.foreground.rgba;
    assert!(color[2] > 0.7 && color[0] < 0.2, "fill ink: {color:?}");
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_gradient_tool() {
    let app = native_test_app("art.capycanvas.GradientTool");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let workspace = tool_settings_workspace(&w, &[CommandId::Gradient], false, false);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.1, 0.25, 0.9, 1.0],
    });
    w.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Select {
            slot: layer_ui::ColorSlot::Background,
        },
    });
    w.dispatch(UiAction::SetColor {
        rgba: [1.0, 0.6, 0.1, 1.0],
    });
    w.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Select {
            slot: layer_ui::ColorSlot::Foreground,
        },
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::Gradient,
    });
    pump(150);
    assert_eq!(w.tool_set.buttons.borrow().len(), 3);
    let opacity = named::<crate::number_control::NumberControl>(&w.panel_widget(Panel::ToolSettings), "tool-setting-opacity");
    edit_number(&opacity, "80");
    assert!((state(&w).brush.opacity - 0.8).abs() < 0.001);
    let dir = artifact_dir("../../artifacts/familiar-workspace");
    for (index, name) in [
        "linear-colors",
        "radial-colors",
        "reflected-colors",
    ]
    .into_iter()
    .enumerate()
    {
        w.dispatch(UiAction::Layer {
            action: LayerAction::Clear { id: 1 },
        });
        pump(50);
        let button = w.tool_set.buttons.borrow()[index].1.clone();
        click(&button);
        let camera = state(&w).camera;
        for (i, (phase, p)) in [
            (PenPhase::Down, [760.0, 620.0]),
            (PenPhase::Move, [1220.0, 850.0]),
            (PenPhase::Up, [1220.0, 850.0]),
        ]
        .into_iter()
        .enumerate()
        {
            let e = pen_event(&camera, p, phase, i as u64);
            w.cursor_input(Some(e));
            w.input.send(&w, e);
            pump(25);
        }
        pump(200);
        assert!(!w.status.is_visible(), "{}", w.status.text());
        let color = |phase| {
            w.input
                .send(&w, pen_event(&camera, [760.0, 620.0], phase, 100))
        };
        // Probe rendered pigment using the real asynchronous GPU path.
        w.dispatch(UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::PickLayer,
            },
        });
        color(PenPhase::Down);
        color(PenPhase::Up);
        pump(150);
        assert!(state(&w).colors.foreground.rgba[2] > 0.85 && state(&w).colors.foreground.rgba[0] < 0.2);
        w.dispatch(UiAction::SetColor {
            rgba: [0.1, 0.25, 0.9, 1.0],
        });
        w.dispatch(UiAction::Invoke {
            command: CommandId::Gradient,
        });
        for theme in [Theme::Dark, Theme::Light] {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            pump(150);
            capture_reference(&w, &format!("{dir}/gradient-{name}-{theme:?}.png"), 1.0);
        }
        w.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        });
        pump(75);
        w.dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        });
        pump(75);
        assert!(!w.status.is_visible(), "{}", w.status.text());
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_navigation_tools() {
    let app = native_test_app("art.capycanvas.NavigationTools");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let mut workspace = state(&w).workspace;
    workspace
        .layout
        .insert_tools(
            Panel::Toolbar,
            None,
            &[
                ToolbarControl::Command {
                    command: CommandId::Hand,
                },
                ToolbarControl::Command {
                    command: CommandId::Eyedropper,
                },
            ],
        )
        .unwrap();
    let ids: Vec<_> = workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .iter()
        .rev()
        .take(2)
        .map(|t| t.id)
        .collect();
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    w.dispatch(UiAction::SelectBrush {
        id: layer_core::DefaultBrushPreset::GPen as u32,
    });
    w.dispatch(UiAction::SetBrushSize { value: 512.0 });
    w.dispatch(UiAction::SetColor {
        rgba: [1.0, 0.0, 0.0, 1.0],
    });
    let contact = |phase| {
        let e = pen_event(&state(&w).camera, [1024.0, 768.0], phase, 0);
        w.cursor_input(Some(e));
        w.input.send(&w, e);
    };
    native_pen_path(&w, &[[1024., 768.], [1024., 768.]]);
    w.dispatch(UiAction::SetLayerOpacity {
        id: None,
        opacity: 0.5,
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.0, 0.0, 0.0, 1.0],
    });
    pump(200);
    let tile = |id| {
        named::<gtk::Button>(w.surface.upcast_ref(), &format!("tile-{id}"))
    };
    let eye = tile(ids[0]);
    click(&eye);
    assert!(eye.tooltip_text().unwrap().ends_with("(I)"));
    assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::PickVisible);
    contact(PenPhase::Down);
    contact(PenPhase::Up);
    pump(250);
    let color = state(&w).colors.foreground.rgba;
    assert!(
        color[0] > 0.99 && (color[1] - 0.5).abs() < 0.015 && (color[2] - 0.5).abs() < 0.015,
        "half-opacity red over white blends perceptually: {color:?}"
    );
    assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::Paint);
    click(&eye);
    w.dispatch(UiAction::ColorPicker {
        action: layer_ui::ColorPickerAction::Source { layer: true },
    });
    assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::PickLayer);
    contact(PenPhase::Down);
    contact(PenPhase::Up);
    pump(250);
    let color = state(&w).colors.foreground.rgba;
    for (actual, expected) in color.into_iter().zip([1.0, 0.0, 0.0, 1.0]) {
        assert!((actual - expected).abs() < 0.001, "raw color {color:?}");
    }
    let startup_pending = !ui_session(&w).engine().backend().startup.complete;
    let idle_start = Instant::now();
    while w.frame_timer.borrow().is_some() && idle_start.elapsed() < Duration::from_secs(5) {
        pump(5);
    }
    let gpu = w.gpu.borrow();
    let g = gpu.as_ref().unwrap();
    assert!(
        w.frame_timer.borrow().is_none(),
        "sampling timer: session={} active={} input={} edits={} present={} startup={}",
        g.session.wants_continuous_frames(), g.session.engine().has_active_stroke(),
        g.session.engine().has_pending_input(), g.session.engine().has_pending_document_edits(),
        g.needs_present, g.session.engine().backend().startup.complete,
    );
    drop(gpu);
    eprintln!("sampling settled after {:.3} ms additional wait (startup pending: {startup_pending})", idle_start.elapsed().as_secs_f64() * 1000.);
    let dir = artifact_dir("../../artifacts/familiar-workspace");
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(200);
        capture_reference(&w, &format!("{dir}/eyedropper-{theme:?}.png"), 1.0);
    }
    click(&tile(ids[1]));
    assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::Hand);
    let camera = state(&w).camera;
    for (phase, position) in [
        (ContactPhase::Down, [600.0, 400.0]),
        (ContactPhase::Move, [700.0, 450.0]),
        (ContactPhase::Up, [700.0, 450.0]),
    ] {
        let reply = w.interact(UiInput::Pointer {
            id: 42,
            phase,
            kind: PointerKind::Mouse,
            button: PointerButton::Primary,
            position,
            time_ns: 0,
        });
        assert!(!reply.paint && reply.pan_cursor);
    }
    pump(200);
    let p = camera.input_transform().map(Point { x: 600.0, y: 400.0 });
    let after = state(&w)
        .camera
        .input_transform()
        .map(Point { x: 700.0, y: 450.0 });
    assert!((p.x - after.x).abs() < 0.001 && (p.y - after.y).abs() < 0.001);
    assert!(!w.status.is_visible(), "{}", w.status.text());
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_navigator_column_resize() {
    let app = native_test_app("art.capycanvas.NavigatorColumnResize");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(1200);
    let original = state(&w).workspace;
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let stats = ui_session(&w)
        .engine()
        .backend()
        .stats
        .clone();
    let assert_visible = |visible| {
        assert_eq!(
            !w.navigator_overviews.placements(&state(&w), 1.).is_empty(),
            visible
        );
        assert_eq!(
            !ui_session(&w)
                .engine()
                .backend()
                .overviews
                .is_empty(),
            visible,
            "GPU placements must follow the panel while the resize is held, without canvas input"
        );
    };
    for panel in [Panel::Brushes, Panel::Layers] {
        let mut workspace = original.clone();
        let group = workspace.layout.panel_group(panel).unwrap();
        workspace
            .layout
            .set_panel_visible(Panel::Navigator, true)
            .unwrap();
        workspace
            .layout
            .move_panel(
                viewport,
                Panel::Navigator,
                DockTarget::Tab { group, index: None },
            )
            .unwrap();
        workspace
            .layout
            .select_tab(group, Panel::Navigator)
            .unwrap();
        let root = workspace.layout.column_for_group(group).unwrap();
        // A selected panel may start as an open member of a collapsed stack.
        // This case exercises an ordinary expanded column and its divider.
        workspace.layout.set_column_collapsed(root, false, viewport).unwrap();
        let band = workspace
            .layout
            .bands
            .iter_mut()
            .find(|b| b.root.id() == root)
            .unwrap();
        band.extent = 400.;
        let id = band.id;
        // This fixture explicitly sizes its columns. Automatic tab-label fits
        // would otherwise raise the collapse threshold above the content
        // minimum used for the held-resize crossings below.
        workspace.layout.fit_tab_groups.clear();
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(workspace),
        });
        pump(400);
        assert_visible(true);
        let d = w
            .resolved()
            .dividers
            .into_iter()
            .find(|d| d.id == id)
            .unwrap();
        let handle = || {
            w.surface
                .imp()
                .children
                .borrow()
                .iter()
                .find(|(slot, _)| *slot == Slot::Divider(id))
                .unwrap()
                .1
                .clone()
        };
        let minimum = if panel == Panel::Brushes {
            192.
        } else {
            layer_ui::LAYERS_MIN_WIDTH
        };
        let outward = if d.reversed { -1. } else { 1. };
        let center = d.bounds.x + d.bounds.width * 0.5;
        let delta = |width| {
            let x = if d.reversed {
                d.parent.x + d.parent.width - width - WORKSPACE_SPACING * 0.5
            } else {
                d.parent.x + width + WORKSPACE_SPACING * 0.5
            };
            [(x - center) as f64, 0.]
        };
        let drag = begin_workspace_drag(&w, &handle(), 1., 20.);
        // Settle at minimum width so reopening has the same projection as
        // before collapse. No canvas hover, explicit wake, or capture is used.
        drag.update(delta(minimum));
        pump(300);
        assert_visible(true);
        for _ in 0..2 {
            let (frames, previews) = {
                let s = stats.lock().unwrap();
                (s.cpu.len(), s.overview_frames)
            };
            drag.update(delta(minimum - TILE_SIZE - 1.));
            pump(200);
            assert!(w.workspace_drag.borrow().is_some());
            assert!(state(&w).workspace.layout.is_collapsed(root));
            assert_visible(false);
            {
                let s = stats.lock().unwrap();
                assert!(
                    s.cpu.len() > frames,
                    "collapse must present a frame without the overview"
                );
                assert_eq!(s.overview_frames, previews);
            }
            drag.update(delta(minimum - TILE_SIZE + 1.));
            pump(200);
            assert_visible(true);
            assert!(
                stats.lock().unwrap().overview_frames > previews,
                "reopening must present the overview before release"
            );
        }
        drag.update(delta(minimum - TILE_SIZE - 1.));
        pump(200);
        assert_visible(false);
        drag.end();
        pump(200);
        // Opening a column that started collapsed has the same rendering lifecycle.
        let drag = begin_workspace_drag(&w, &handle(), 1., 20.);
        for (distance, visible) in [(36., true), (35., false), (36., true)] {
            let previews = stats.lock().unwrap().overview_frames;
            drag.update([outward * distance, 0.]);
            pump(200);
            assert_visible(visible);
            if visible {
                assert!(stats.lock().unwrap().overview_frames > previews);
            }
        }
        // Holding the mouse stationary must not keep rendering after the update.
        pump(250);
        let frames = stats.lock().unwrap().cpu.len();
        pump(250);
        assert_eq!(stats.lock().unwrap().cpu.len(), frames);
        drag.end();
    }
    assert!(!w.status.is_visible(), "{}", w.status.text());
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_navigator() {
    let app = native_test_app("art.capycanvas.NavigatorReview");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(800);
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetPanelVisible {
            panel: Panel::Navigator,
            visible: true,
        },
    });
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut workspace = state(&w).workspace;
    let layers = workspace.layout.panel_group(Panel::Layers).unwrap();
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::Navigator,
            DockTarget::Split {
                group: layers,
                edge: Edge::Top,
            },
        )
        .unwrap();
    let navigator = workspace.layout.panel_group(Panel::Navigator).unwrap();
    workspace
        .layout
        .set_panel_visible(Panel::Stats, true)
        .unwrap();
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::Stats,
            DockTarget::Tab {
                group: navigator,
                index: None,
            },
        )
        .unwrap();
    workspace
        .layout
        .select_tab(navigator, Panel::Navigator)
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(250);
    w.dispatch(UiAction::SetBrushSize { value: 96.0 });
    let mut sequence = 0;
    for (row, rgba) in [
        [0.75, 0.18, 0.2, 1.0],
        [0.18, 0.55, 0.36, 1.0],
        [0.2, 0.36, 0.75, 1.0],
    ]
    .into_iter()
    .enumerate()
    {
        w.dispatch(UiAction::SetColor { rgba });
        let camera = state(&w).camera;
        for i in 0..32 {
            let x = 200.0 + i as f32 * 50.0;
            let y = 350.0 + row as f32 * 350.0 + (i as f32 * 0.2).sin() * 120.0;
            sequence += 1;
            let phase = match i {
                0 => PenPhase::Down,
                31 => PenPhase::Up,
                _ => PenPhase::Move,
            };
            let event = pen_event(&camera, [x, y], phase, sequence);
            let mut gpu = w.gpu.borrow_mut();
            let session = &mut gpu.as_mut().unwrap().session;
            session
                .pen(PenEvent {
                    device_id: 91,
                    ..event
                })
                .unwrap();
        }
        w.wake();
        pump(250);
    }
    pump(400);
    let original = ui_session(&w)
        .engine()
        .backend()
        .stats
        .lock()
        .unwrap()
        .overview_revisions
        .clone();
    assert!(
        !original.is_empty(),
        "live document overview must be presented by the worker"
    );
    assert_eq!(w.navigator_overviews.placements(&state(&w), 1.).len(), 1);
    let doc_revision = ui_session(&w)
        .engine()
        .document()
        .revision;
    let zoom = state(&w).camera.zoom;
    let button = |id: CommandId| {
        named::<gtk::Button>(w.navigator.root.upcast_ref(), &format!("navigator-{id:?}"))
    };
    for _ in 0..3 {
        click(&button(CommandId::ZoomIn));
    }
    click(&button(CommandId::RotateRight));
    click(&button(CommandId::FlipHorizontal));
    pump(300);
    assert!((state(&w).camera.zoom - zoom * 8.0_f32.sqrt()).abs() < 0.001);
    assert_eq!(state(&w).camera.flipped, [true, false]);
    assert!(button(CommandId::FlipHorizontal).has_css_class("selected-tool"));
    assert_eq!(
        ui_session(&w)
            .engine()
            .backend()
            .stats
            .lock()
            .unwrap()
            .overview_revisions,
        original,
        "camera-only commands reuse the exact document composition"
    );
    assert_eq!(
        ui_session(&w)
            .engine()
            .document()
            .revision,
        doc_revision
    );
    let overview = find_named(w.navigator.root.upcast_ref(), "navigator-overview").unwrap();
    assert!(
        w.navigator.root.measure(gtk::Orientation::Horizontal, -1).0 <= 192,
        "compact control row must fit minimum panel width"
    );
    let size = [overview.width() as f32, overview.height() as f32];
    let g = NavigatorGeometry::new(&state(&w).camera, [2048, 1536], size).unwrap();
    let position = [
        g.image.x + g.image.width * 0.3,
        g.image.y + g.image.height * 0.65,
    ];
    w.dispatch(UiAction::Navigator {
        phase: ContactPhase::Down,
        position,
        viewport: size,
    });
    w.dispatch(UiAction::Navigator {
        phase: ContactPhase::Move,
        position: [position[0] + 10.0, position[1]],
        viewport: size,
    });
    w.dispatch(UiAction::Navigator {
        phase: ContactPhase::Up,
        position,
        viewport: size,
    });
    pump(200);
    let dir = artifact_dir("../../artifacts/familiar-workspace");
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreSettings {
            settings: Settings {
                theme: Some(theme),
                ..Settings::default()
            },
        });
        pump(250);
        capture_reference(&w, &format!("{dir}/navigator-{theme:?}.png"), 1.0);
    }
    // A second projection shares the same image/cache producer.
    let mut workspace = state(&w).workspace;
    workspace
        .layout
        .insert_tools(
            Panel::Toolbar,
            None,
            &[ToolbarControl::Panel {
                panel: Panel::Navigator,
            }],
        )
        .unwrap();
    let tile = workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .last()
        .unwrap()
        .id;
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(150);
    click(
        &named::<gtk::Button>(w.surface.upcast_ref(), &format!("tile-{tile}")),
    );
    pump(300);
    assert!(
        find_named(w.surface.upcast_ref(), "drawer-panel-Navigator")
            .unwrap()
            .is_mapped()
    );
    assert_eq!(
        w.navigator_overviews.placements(&state(&w), 1.).len(),
        2,
        "drawer and dock share the live composition"
    );
    capture_reference(&w, &format!("{dir}/navigator-drawer.png"), 1.0);
    click(
        &named::<gtk::Button>(w.surface.upcast_ref(), &format!("tile-{tile}")),
    );
    pump(300);
    assert_eq!(w.navigator_overviews.placements(&state(&w), 1.).len(), 1);
    // CSS opacity is separate from Widget::opacity. The GPU image must disappear
    // with the docked panel, then return without exporting any image pixels.
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    pump(300);
    assert!(w.navigator_overviews.placements(&state(&w), 1.).is_empty());
    capture_reference(&w, &format!("{dir}/navigator-zen.png"), 1.0);
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    pump(300);
    assert_eq!(w.navigator_overviews.placements(&state(&w), 1.).len(), 1);
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Navigator,
        viewport,
        target: DockTarget::Float {
            position: [20., 180.],
        },
    });
    pump(350);
    let brushes = w
        .panel_widget(Panel::Brushes)
        .compute_bounds(&w.surface)
        .unwrap();
    let p = w.navigator_overviews.placements(&state(&w), 1.)[0];
    // Place the image over today's allocated panel, including the actual
    // letterbox/header offset. Fixed document-era window coordinates can miss
    // the panel after workspace geometry changes.
    let grip = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.panels.contains(&Panel::Navigator))
        .unwrap()
        .bounds;
    let float_position = [
        grip.x + grip.width * 0.5 + brushes.x() + brushes.width() * 0.5 - (p.bounds[0] + 8.),
        grip.y + TAB_BAR_HEIGHT * 0.5 + brushes.y() + brushes.height() * 0.5 - (p.bounds[1] + 8.),
    ];
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Navigator,
        viewport,
        target: DockTarget::Float { position: float_position },
    });
    pump(350);
    let p = w.navigator_overviews.placements(&state(&w), 1.)[0];
    let sample = [p.bounds[0] + 8., p.bounds[1] + 8.];
    assert!(
        brushes.contains_point(&gtk::graphene::Point::new(sample[0], sample[1])),
        "floating image must overlap an actual native panel"
    );
    let texture = crate::snapshot(&w);
    let mut pixels = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut pixels, texture.width() as usize * 4);
    let offset = (sample[1] as usize * texture.width() as usize + sample[0] as usize) * 4;
    assert!(
        pixels[offset..offset + 3].iter().all(|v| *v > 240),
        "the floating overview's white paper must cover the native panel below: {:?}",
        &pixels[offset..offset + 4]
    );
    capture_reference(&w, &format!("{dir}/navigator-over-panel.png"), 1.0);
    // Reversing the overlap must preserve native controls above the image.
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Sizes,
        viewport,
        target: DockTarget::Float {
            position: float_position,
        },
    });
    pump(350);
    let sizes = w.panel_widget(Panel::Sizes).compute_bounds(&w.surface).unwrap();
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Sizes,
        viewport,
        target: DockTarget::Float {
            position: [
                float_position[0] + sample[0] - (sizes.x() + 2.),
                float_position[1] + sample[1] - (sizes.y() + 2.),
            ],
        },
    });
    pump(350);
    assert!(w.panel_widget(Panel::Sizes).compute_bounds(&w.surface).unwrap()
        .contains_point(&gtk::graphene::Point::new(sample[0], sample[1])));
    let texture = crate::snapshot(&w);
    texture.download(&mut pixels, texture.width() as usize * 4);
    assert!(
        pixels[offset..offset + 3].iter().all(|v| *v < 160),
        "a later native panel must cover the overview: {:?}",
        &pixels[offset..offset + 4]
    );
    capture_reference(&w, &format!("{dir}/navigator-under-panel.png"), 1.0);
    // No preview timer or GTK frame clock should keep an idle window rendering.
    pump(500);
    let frames = || {
        ui_session(&w)
            .engine()
            .backend()
            .stats
            .lock()
            .unwrap()
            .overview_frames
    };
    let before_idle = frames();
    pump(250);
    assert_eq!(
        frames(),
        before_idle,
        "idle Navigator must not schedule frames"
    );
    assert!(!w.status.is_visible(), "{}", w.status.text());
    w.window.close();
    pump(80);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_tool_families() {
    let app = native_test_app("art.capycanvas.ToolFamilies");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(800);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut workspace = state(&w).workspace;
    let toolbar = workspace.layout.panels.iter_mut().find(|p| p.id == Panel::Toolbar).unwrap();
    if let layer_ui::PanelContent::Toolbar { tiles, .. } = &mut toolbar.content {
        tiles.clear();
    }
    workspace.layout.insert_tools(Panel::Toolbar, None, &layer_ui::Tool::ALL.map(|tool| {
        layer_ui::ToolbarControl::Command { command: tool.command() }
    })).unwrap();
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::Toolbar,
            DockTarget::Edge {
                edge: Edge::Left,
                outer: true,
            },
        )
        .unwrap();
    workspace
        .layout
        .set_panel_visible(Panel::ToolSettings, true)
        .unwrap();
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::ToolSettings,
            DockTarget::Float {
                position: [500., 120.],
            },
        )
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(150);
    let output = artifact_dir("../../artifacts/familiar-workspace");
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for tool in layer_ui::Tool::ALL {
            click(&command(&w, tool.command()));
            if state(&w).customization.drawer.is_some() {
                w.dispatch(UiAction::Customize {
                    action: CustomizationAction::CloseExpanded,
                });
                pump(220);
            }
            pump(80);
            assert_eq!(state(&w).brush.tool, tool);
            let groups = state(&w).tool_set.groups;
            for (index, _) in groups.iter().enumerate() {
                // Activate actual native group and brush buttons, not just the
                // state API. Copy the widget before invoking its callback.
                let button = w.tool_set.group_buttons.borrow()[index].clone();
                click(&button);
                pump(30);
                assert!(button.has_css_class("selected-tool"));
                let bounds = button.compute_bounds(&w.tool_set.root).unwrap();
                assert_eq!(
                    bounds.width(),
                    layer_ui::TOOL_PANEL_MIN_WIDTH - 2. * layer_ui::PANEL_CONTENT_INSET
                );
                assert_eq!(bounds.height(), layer_ui::TILE_SIZE);
                let buttons = w.tool_set.buttons.borrow().clone();
                for (item, button, _) in buttons {
                    click(&button);
                    pump(20);
                    assert_eq!(Some(state(&w).brush.preset), item.preview);
                    assert!(button.has_css_class("selected-tool"));
                    assert_eq!(
                        w.tool_set
                            .buttons
                            .borrow()
                            .iter()
                            .filter(|(_, b, _)| b.has_css_class("selected-tool"))
                            .count(),
                        1
                    );
                    let snapshot = ui_session(&w)
                        .engine()
                        .configured_brush()
                        .clone();
                    assert_eq!(snapshot.diameter, state(&w).brush.diameter);
                    assert_eq!(snapshot.opacity, state(&w).brush.opacity);
                    if tool == layer_ui::Tool::Liquify {
                        assert_eq!(
                            snapshot.execution_class(),
                            layer_core::BrushExecution::Liquify
                        );
                    }
                }
            }
            capture_reference(&w, &format!("{output}/tool-{tool:?}-{theme:?}.png"), 1.0);
        }
    }
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_tool_and_color_panels() {
    let app = native_test_app("art.capycanvas.ToolPanels");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(800);
    let mut workspace = state(&w).workspace;
    workspace
        .layout
        .set_panel_visible(Panel::ToolSettings, true)
        .unwrap();
    workspace
        .layout
        .set_panel_visible(Panel::Color, true)
        .unwrap();
    for (panel, x) in [(Panel::ToolSettings, 330.), (Panel::Color, 610.)] {
        workspace
            .layout
            .move_panel(
                [1200., 900.],
                panel,
                DockTarget::Float {
                    position: [x, 130.],
                },
            )
            .unwrap();
    }
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(200);
    for choice in layer_ui::brush_catalog() {
        w.dispatch(UiAction::SelectBrush { id: choice.id });
        pump(10);
        assert!(
            w.panel_widget(Panel::ToolSettings)
                .measure(gtk::Orientation::Horizontal, -1)
                .0
                <= layer_ui::TOOL_PANEL_MIN_WIDTH as i32,
            "{} settings are too wide",
            choice.label
        );
    }
    assert!(
        w.panel_widget(Panel::Brushes)
            .measure(gtk::Orientation::Horizontal, -1)
            .0
            <= layer_ui::TOOL_PANEL_MIN_WIDTH as i32
    );
    w.dispatch(UiAction::SelectBrush {
        id: layer_core::DefaultBrushPreset::WetWatercolor as u32,
    });
    let flow = named::<crate::number_control::NumberControl>(&w.panel_widget(Panel::ToolSettings), "tool-setting-flow");
    edit_number(&flow, "25+10");
    assert!(
        (state(&w)
            .tool_settings
            .iter()
            .find(|s| s.id == "flow")
            .unwrap()
            .value
            - 0.35)
            .abs()
            < 1e-6
    );
    let color = w.panel_widget(Panel::Color);
    click(
        &find_named(&color, "color-Background")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert_eq!(state(&w).colors.slot, layer_ui::ColorSlot::Background);
    click(
        &find_named(&color, "color-Transparent")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert!(state(&w).colors.transparent());
    let before = state(&w).colors.rgba();
    click(
        &find_named(&color, "color-readout")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert_eq!(state(&w).colors.readout, layer_ui::ColorReadout::Rgb);
    assert_eq!(state(&w).colors.rgba(), before);
    for expected in [
        layer_ui::ColorShape::Square,
        layer_ui::ColorShape::Triangle,
        layer_ui::ColorShape::Circle,
    ] {
        let index = state(&w)
            .colors
            .other_shapes()
            .iter()
            .position(|shape| *shape == expected)
            .unwrap();
        click(
            &find_named(&color, &format!("color-shape-{index}"))
                .unwrap()
                .downcast()
                .unwrap(),
        );
        assert_eq!(state(&w).colors.shape, expected);
    }
    let before = state(&w).colors;
    click(
        &find_named(&color, "color-swap")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert_eq!(state(&w).colors.foreground, before.background);
    assert_eq!(state(&w).colors.background, before.foreground);
    click(
        &find_named(&color, "color-Foreground")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    w.dispatch(UiAction::SetColor {
        rgba: [0.2, 0.72, 0.34, 1.],
    });
    let output = std::path::Path::new("../../artifacts/familiar-workspace");
    std::fs::create_dir_all(output).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(100);
        for shape in [layer_ui::ColorShape::Square, layer_ui::ColorShape::Triangle] {
            w.dispatch(UiAction::Color {
                action: layer_ui::ColorAction::Shape { shape },
            });
            pump(100);
            capture_reference(
                &w,
                output
                    .join(format!("color-{shape:?}-{theme:?}.png"))
                    .to_str()
                    .unwrap(),
                1.0,
            );
        }
    }
    let mut workspace = state(&w).workspace;
    workspace
        .layout
        .move_panel(
            [1200., 900.],
            Panel::Brushes,
            DockTarget::Float {
                position: [320., 150.],
            },
        )
        .unwrap();
    for float in &mut workspace.layout.floating {
        if let DockNode::Tabs { panels, .. } = &float.root
            && (panels.contains(&Panel::Brushes) || panels.contains(&Panel::ToolSettings))
        {
            float.width = layer_ui::TOOL_PANEL_MIN_WIDTH;
            float.height = Some(600.0);
            float.position = [
                if panels.contains(&Panel::Brushes) {
                    260.0
                } else {
                    400.0
                },
                120.0,
            ];
        } else {
            float.position = [600.0, 120.0];
        }
    }
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(150);
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(100);
        capture_reference(
            &w,
            output
                .join(format!("three-tile-minimum-{theme:?}.png"))
                .to_str()
                .unwrap(),
            1.0,
        );
    }
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_runtime_filter_packages() {
    let app = native_test_app("art.capycanvas.RuntimeFilters");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(900);
    let load = |path: &str, mode| {
        crate::canvas::load_filter_directory(
            &mut ui_session_mut(&w),
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path),
            mode,
        )
        .unwrap();
        w.wake();
        let deadline = Instant::now() + Duration::from_secs(20);
        while state(&w).filter_load.pending && Instant::now() < deadline {
            pump(20);
        }
        assert!(!state(&w).filter_load.pending);
        assert!(
            state(&w).filter_load.error.is_none(),
            "{:?}",
            state(&w).filter_load.error
        );
    };
    let bundled = layer_core::bundled_effect_catalog().filters().len();
    load("../../assets/filters", layer_core::EffectInstallMode::Merge);
    assert_eq!(state(&w).adjustments.len(), bundled);
    load(
        "../../examples/filters/tent-blur",
        layer_core::EffectInstallMode::Merge,
    );
    assert_eq!(state(&w).adjustments.len(), bundled + 1);
    let checker = layer_core::color::source::rgba8_source([1024, 768], |x, y| {
        if (x / 32 + y / 32) % 2 == 0 {
            [230, 50, 80, 255]
        } else {
            [30, 160, 220, 255]
        }
    });
    ui_session_mut(&w)
        .import_layer_source("Runtime checker", std::sync::Arc::unwrap_or_clone(checker))
        .unwrap();
    w.wake();
    pump(100);
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::Category {
            category: Some("examples".into()),
        },
    });
    w.dispatch(UiAction::SelectPanelTab {
        group: 8,
        panel: Panel::Adjustments,
    });
    pump(300);
    named::<gtk::Button>(w.effects.adjustments.upcast_ref(), "adjustment-example:tent_blur")
    .emit_clicked();
    pump(300);
    let view = state(&w).layer_properties;
    assert_eq!(view.description, "Tent Blur");
    assert_eq!(view.controls[0].label, "Radius");
    assert!(view.enabled);
    w.dispatch(UiAction::Effect {
        action: layer_ui::EffectAction::Set {
            layer: view.layer.unwrap(),
            key: "radius".into(),
            value: layer_core::EffectValue::Number(9.),
        },
    });
    pump(150);
    load(
        "../../examples/filters/tent-blur",
        layer_core::EffectInstallMode::Replace,
    );
    assert_eq!(
        state(&w).layer_properties.controls[0].value,
        layer_core::EffectValue::Number(9.)
    );
    let dir = artifact_dir("../../artifacts/ui/runtime-filters-gtk");
    crate::capture(&w, &format!("{dir}/runtime-properties.png"));
    w.window.close();
    pump(80);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_adjustment_panels_review() {
    use layer_core::EffectValue;
    use layer_ui::EffectAction;
    let app = native_test_app("art.capycanvas.AdjustmentReview");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(900);
    let show_adjustments = || {
        let group = w
            .resolved()
            .groups
            .into_iter()
            .find(|g| g.panels.contains(&Panel::Adjustments))
            .unwrap();
        if group.active != Panel::Adjustments {
            w.dispatch(UiAction::SelectPanelTab {
                group: group.id,
                panel: Panel::Adjustments,
            });
        }
        pump(80);
        assert!(w.effects.adjustments.is_mapped());
    };
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    apply_fixture_theme(&w);
    let study = layer_core::color::source::rgba8_source([512, 512], |x, y| {
        let (x, y) = (x as f32 / 511., y as f32 / 511.);
        [
            (x * 255.) as u8,
            (y * 255.) as u8,
            ((1. - x) * 255.) as u8,
            255,
        ]
    });
    ui_session_mut(&w)
        .import_layer_source("Color study", std::sync::Arc::unwrap_or_clone(study))
        .unwrap();
    w.refresh(regions::ALL);
    w.wake();
    pump(300);
    let dir = artifact_dir("../../artifacts/ui/adjustments-gtk");
    show_adjustments();
    pump(900);
    crate::capture(&w, &format!("{dir}/01-adjustments.png"));
    let first = find_named(w.effects.adjustments.upcast_ref(), "adjustment-curves").unwrap();
    let picture = first
        .downcast_ref::<gtk::Button>()
        .unwrap()
        .child()
        .unwrap()
        .first_child()
        .unwrap()
        .downcast::<gtk::Picture>()
        .unwrap();
    let preview_deadline = Instant::now() + Duration::from_secs(20);
    while picture.paintable().is_none() {
        assert!(
            Instant::now() < preview_deadline,
            "visible filter rows receive asynchronous GPU previews"
        );
        pump(30);
    }
    let second = find_named(w.effects.adjustments.upcast_ref(), "adjustment-levels").unwrap();
    assert!(first.width() > 160);
    assert!(
        second.compute_bounds(&w.effects.adjustments).unwrap().y()
            > first.compute_bounds(&w.effects.adjustments).unwrap().y()
    );
    for (i, kind) in layer_core::bundled_effect_catalog()
        .filters()
        .iter()
        .enumerate()
    {
        show_adjustments();
        pump(80);
        named::<gtk::Button>(w.effects.adjustments.upcast_ref(), &format!("adjustment-{}", kind.id()))
        .emit_clicked();
        new_photo::ready(&w);
        let updating=w.localization().text(layer_ui::localization::MessageId::RESOURCES_ANALYSIS_UPDATING);
        until(||state(&w).layer_properties.description!=updating.as_ref(),"adjustment source analysis completed");
        assert!(!w.status.is_visible(), "{}", w.status.text());
        assert!(w.effects.properties.is_mapped());
        let s = state(&w);
        let id = s.layer_properties.layer.unwrap();
        let expected = match kind.label() {
            layer_core::ResourceLabel::Literal(text) => text.clone(),
            layer_core::ResourceLabel::Message { message } => {
                let session = ui_session(&w);
                session.localization().text(session.localization().static_message(message).unwrap())
            }
        };
        assert_eq!(s.layer_properties.description, expected.as_ref());
        if kind.id() == "color_balance" {
            let pages=named::<gtk::DropDown>(w.effects.properties.upcast_ref(),"properties-page");
            let model=pages.model().unwrap();
            assert_eq!((0..model.n_items()).map(|i|model.item(i).unwrap().downcast::<gtk::StringObject>().unwrap().string().to_string()).collect::<Vec<_>>(),["Shadows","Midtones","Highlights"]);
        }
        let program = kind.program();
        let edit = (!program.parameters.is_empty()).then(|| match kind.id() {
            "curves" => (
                "rgb",
                EffectValue::Curve(vec![[0., 0.], [0.4, 0.65], [1., 1.]]),
            ),
            "levels" => ("gamma", EffectValue::Number(1.5)),
            "brightness_contrast" => ("contrast", EffectValue::Number(30.)),
            "hue_saturation" => ("hue", EffectValue::Number(40.)),
            "color_balance" => ("midtones_red", EffectValue::Number(25.)),
            "exposure" => ("exposure", EffectValue::Number(1.)),
            "vibrance" => ("vibrance", EffectValue::Number(75.)),
            "black_white" => ("reds", EffectValue::Number(80.)),
            "gradient_map" => (
                "gradient",
                EffectValue::Gradient(layer_core::GradientDefinition::new(vec![
                    layer_core::GradientStop {
                        position: 0.,
                        color: layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [0.03, 0.05, 0.2, 1.]).unwrap(),
                    },
                    layer_core::GradientStop {
                        position: 0.5,
                        color: layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [0.8, 0.2, 0.1, 1.]).unwrap(),
                    },
                    layer_core::GradientStop {
                        position: 1.,
                        color: layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [1., 0.9, 0.5, 1.]).unwrap(),
                    },
                ])),
            ),
            "posterize" => ("levels", EffectValue::Number(4.)),
            "solid_color" => (
                "color",
                EffectValue::Color(layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [0.8, 0.2, 0.1, 1.]).unwrap()),
            ),
            _ => {
                let parameter = program
                    .parameters
                    .iter()
                    .find(|p| matches!(p.kind, layer_core::EffectParameterKind::Number { .. }))
                    .unwrap();
                let layer_core::EffectParameterKind::Number { min, max, .. } = parameter.kind
                else {
                    unreachable!()
                };
                let value=min+(max-min)*0.3;
                (parameter.key.as_ref(),EffectValue::Number(if parameter.dimension==layer_core::authored::Dimension::Count {value.round()} else {value}))
            }
        });
        if let Some((key,value))=edit {
            w.dispatch(UiAction::Effect {
                action: EffectAction::Set {
                    layer: id,
                    key: key.into(),
                    value,
                },
            });
            pump(200);
            assert!(!w.status.is_visible(), "{}", w.status.text());
        }
        if kind.id() == "levels" {
            for (key, label) in [("clamp_input", "Clamp input"), ("clamp_output", "Clamp output")] {
                let mut child = w.effects.properties.last_child().unwrap().first_child();
                let mut input = None;
                while let Some(widget) = child {
                    if widget.first_child().and_downcast::<gtk::Label>()
                        .is_some_and(|title| title.text() == label) {
                        input = widget.last_child().and_downcast::<gtk::Switch>();
                        break;
                    }
                    child = widget.next_sibling();
                }
                let input = input.expect("Levels clipping switch");
                assert!(!input.is_active());
                for active in [true, false] {
                    input.set_active(active);
                    pump(30);
                    assert_eq!(state(&w).layer_properties.controls.iter()
                        .find(|c| c.key == key).unwrap().value, EffectValue::Toggle(active));
                }
            }
            let scroll = w.effects.properties.ancestor(gtk::ScrolledWindow::static_type())
                .unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
            let adjustment = scroll.vadjustment();
            adjustment.set_value(adjustment.upper() - adjustment.page_size());
            pump(80);
            crate::capture(&w, &format!("{dir}/03-levels-clipping.png"));
            adjustment.set_value(0.);
            pump(40);
        }
        if kind.id() == "gradient_map" {
            let bar = find_named(w.effects.properties.upcast_ref(), "effect-gradient").unwrap();
            histogram::scroll_to(&bar);
            let mut input=RemoteInput::new().timeout_secs(30);input.ready();
            input.click(screen_point(&bar,&w.window,[0.3,0.5]));input.finish();
            pump(80);
            let EffectValue::Gradient(stops) = &state(&w).layer_properties.controls[0].value else {
                panic!("gradient control")
            };
            assert_eq!(
                stops.stops.len(),
                4,
                "native gradient insertion is handled by Rust"
            );
        }
        crate::capture(&w, &format!("{dir}/{:02}-{}.png", i + 2, kind.id()));
        w.dispatch(UiAction::SetLayerVisibility { id, visible: false });
    }
    // Category/search changes use the shared policy and preserve the selected
    // editing layer. The GTK view only rebuilds the matching rows.
    show_adjustments();
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::Category {
            category: Some("distort".into()),
        },
    });
    pump(700);
    crate::capture(&w, &format!("{dir}/41-distort-picker.png"));
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::ToggleSearch,
    });
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::Search {
            query: "glass".into(),
        },
    });
    pump(700);
    assert_eq!(state(&w).adjustments.len(), 2);
    crate::capture(&w, &format!("{dir}/42-glass-search.png"));
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::ToggleSearch,
    });
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::Category { category: None },
    });
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetPanelVisible {
            panel: Panel::Stats,
            visible: true,
        },
    });
    pump(300);
    // Keep the stable stats exercise on a simple pointwise filter.
    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: "posterize".into(),
        },
    });
    let layer = state(&w).layer_properties.layer.unwrap();
    w.dispatch(UiAction::SetLayerVisibility {
        id: layer,
        visible: true,
    });
    // Inserting an effect selects Properties. Diagnostics collect only while
    // their panel is active, so activate it before measuring the edits.
    let stats_group = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.panels.contains(&Panel::Stats))
        .unwrap();
    if stats_group.active != Panel::Stats {
        w.dispatch(UiAction::SelectPanelTab {
            group: stats_group.id,
            panel: Panel::Stats,
        });
    }
    pump(100);
    assert!(w.effects.stats.is_mapped());
    for i in 0..16 {
        w.dispatch(UiAction::Effect {
            action: EffectAction::Set {
                layer,
                key: "levels".into(),
                value: EffectValue::Number(i as f32 + 2.),
            },
        });
        pump(20);
    }
    pump(250);
    let stats = ui_session(&w).renderer_stats();
    assert!(!stats.samples.is_empty(), "live CPU samples");
    assert_ne!(stats.rows[1].value, "—", "live GPU timestamps");
    crate::capture(&w, &format!("{dir}/07-stats.png"));
    w.window.close();
    pump(80);
}

#[test]
#[ignore = "requires the private Wayland display and hardware GPU"]
fn native_layer_panel_review() {
    use layer_ui::{LayerAction as A, LayerCanvasTool as T};
    fn send(w: &Rc<Workspace>, action: A) {
        w.dispatch(UiAction::Layer { action });
        pump(60);
        assert!(!w.status.is_visible(), "{}", w.status.text());
    }
    fn polygon(w: &Rc<Workspace>, points: &[[f32; 2]], tool: T) {
        send(w, A::Tool { tool });
        let camera = state(w).camera;
        for (i, p) in points.iter().enumerate() {
            let phase = match i {
                0 => PenPhase::Down,
                _ if i + 1 == points.len() => PenPhase::Up,
                _ => PenPhase::Move,
            };
            let event = pen_event(&camera, *p, phase, i as u64 + 1);
            let mut gpu = w.gpu.borrow_mut();
            gpu.as_mut()
                .unwrap()
                .session
                .pen(PenEvent {
                    device_id: 91,
                    ..event
                })
                .unwrap();
        }
        w.wake();
        pump(180);
        assert!(!w.status.is_visible(), "{}", w.status.text());
    }
    let app = native_test_app("art.capycanvas.LayerPanelReview");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let delete = named::<gtk::Button>(w.layer_panel.root.upcast_ref(), "delete-selected-layers");
    let count = state(&w).layers.len();
    send(
        &w,
        A::New {
            group: false,
            clipped: false,
        },
    );
    assert!(delete.is_sensitive());
    delete.emit_clicked();
    pump(80);
    assert_eq!(state(&w).layers.len(), count);
    w.dispatch(UiAction::SelectLayer { id: 2 });
    assert!(delete.is_sensitive());
    w.dispatch(UiAction::SelectLayer { id: 1 });
    let dir = artifact_dir("../../artifacts/ui/layers-gtk");
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    let names = [
        ("Atmosphere", [0.77, 0.86, 0.83, 1.]),
        ("Far hills", [0.35, 0.54, 0.49, 1.]),
        ("Shirt base", [0.65, 0.32, 0.24, 1.]),
        ("Skin base", [0.79, 0.62, 0.40, 1.]),
    ];
    for (i, (name, color)) in names.iter().enumerate() {
        if i > 0 {
            send(
                &w,
                A::New {
                    group: false,
                    clipped: false,
                },
            );
        }
        let id = state(&w).layers.iter().find(|l| l.selected).unwrap().id;
        send(
            &w,
            A::Rename {
                id,
                name: (*name).into(),
            },
        );
        w.dispatch(UiAction::SetColor { rgba: *color });
        let points = match i {
            0 => vec![[180., 160.], [1800., 160.], [1800., 1360.], [180., 1360.]],
            1 => vec![
                [180., 900.],
                [500., 540.],
                [860., 740.],
                [1350., 390.],
                [1800., 850.],
                [1800., 1360.],
                [180., 1360.],
            ],
            2 => vec![
                [700., 690.],
                [980., 640.],
                [1150., 980.],
                [1110., 1350.],
                [560., 1350.],
                [550., 950.],
            ],
            _ => (0..48)
                .map(|i| {
                    let a = i as f32 / 48. * std::f32::consts::TAU;
                    [830. + a.cos() * 195., 500. + a.sin() * 260.]
                })
                .collect(),
        };
        polygon(&w, &points, T::LassoFill);
        send(&w, A::AlphaLock { id, value: true });
    }
    send(
        &w,
        A::New {
            group: false,
            clipped: true,
        },
    );
    let shading = state(&w).layers.iter().find(|l| l.selected).unwrap().id;
    send(
        &w,
        A::Rename {
            id: shading,
            name: "Skin · soft shadow".into(),
        },
    );
    send(
        &w,
        A::Blend {
            id: shading,
            value: 1,
        },
    );
    w.dispatch(UiAction::SetColor {
        rgba: [0.32, 0.25, 0.21, 0.6],
    });
    polygon(
        &w,
        &[[810., 200.], [1100., 200.], [1100., 820.], [900., 800.]],
        T::LassoFill,
    );
    let fabric = layer_core::color::source::rgba8_source([2048, 1536], |x, y| {
        if (x / 12 + y / 12) % 2 == 0 {
            [220, 170, 66, 255]
        } else {
            [178, 124, 42, 255]
        }
    });
    ui_session_mut(&w)
        .import_layer_source("Fabric texture", std::sync::Arc::unwrap_or_clone(fabric))
        .unwrap();
    w.wake();
    pump(200);
    let texture = state(&w).layers.iter().find(|l| l.selected).unwrap().id;
    polygon(
        &w,
        &[[640., 870.], [940., 810.], [1080., 1280.], [680., 1260.]],
        T::Select,
    );
    send(
        &w,
        A::AddMask {
            id: texture,
            replace: false,
        },
    );
    send(
        &w,
        A::LinkMask {
            id: texture,
            value: false,
        },
    );
    pump(650);
    let minimum = w
        .layer_panel
        .root
        .measure(gtk::Orientation::Horizontal, -1)
        .0;
    assert!(
        minimum <= layer_ui::LAYERS_MIN_WIDTH as i32,
        "Layer widgets require {minimum}px"
    );
    capture_reference(&w, &format!("{dir}/01-compact-dark.png"), 1.);
    send(
        &w,
        A::ShowMask {
            id: texture,
            value: true,
        },
    );
    pump(200);
    capture_reference(&w, &format!("{dir}/02-mask-overlay.png"), 1.);
    send(
        &w,
        A::ShowMask {
            id: texture,
            value: false,
        },
    );
    send(&w, A::ApplyMask { id: texture });
    pump(200);
    capture_reference(&w, &format!("{dir}/03-applied-mask.png"), 1.);
    click(&command(&w, CommandId::Undo));
    assert!(
        state(&w)
            .layers
            .iter()
            .find(|l| l.id == texture)
            .unwrap()
            .has_mask
    );
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Light),
    });
    pump(300);
    capture_reference(&w, &format!("{dir}/04-compact-light.png"), 1.);
    // Exercise the native row controls, including virtual-row identity across
    // selection changes (double clicks must not lose their GTK gesture).
    let row = |id| find_named(w.layer_panel.root.upcast_ref(), &format!("art-layer-{id}")).unwrap();
    let label = find_css(&row(1), "layer-name").unwrap();
    let controllers = label.observe_controllers();
    let rename = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
        .find(|g| g.button() == 1)
        .unwrap();
    rename.emit_by_name::<()>("pressed", &[&1i32, &5f64, &5f64]);
    pump(100);
    assert_eq!(label, find_css(&row(1), "layer-name").unwrap());
    rename.emit_by_name::<()>("pressed", &[&2i32, &5f64, &5f64]);
    pump(100);
    let entry: gtk::Entry = find_css(&row(1), "layer-name-entry")
        .unwrap()
        .downcast()
        .unwrap();
    assert!(entry.is_mapped());
    entry.set_text("Atmosphere wash");
    capture_reference(&w, &format!("{dir}/06-inline-rename.png"), 1.);
    entry.emit_activate();
    pump(100);
    assert_eq!(
        state(&w).layers.iter().find(|l| l.id == 1).unwrap().label,
        "Atmosphere wash"
    );
    send(
        &w,
        A::Select {
            id: texture,
            mask: true,
        },
    );
    send(
        &w,
        A::New {
            group: true,
            clipped: false,
        },
    );
    let outer = state(&w).layers.iter().find(|l| l.editing).unwrap().id;
    send(
        &w,
        A::Rename {
            id: outer,
            name: "Character".into(),
        },
    );
    send(
        &w,
        A::New {
            group: true,
            clipped: false,
        },
    );
    let inner = state(&w).layers.iter().find(|l| l.editing).unwrap().id;
    send(
        &w,
        A::Rename {
            id: inner,
            name: "Fabric details".into(),
        },
    );
    send(&w, A::Collapse { id: inner });
    let source_row = row(texture);
    let preview = crate::layers::drag_preview(&source_row, state(&w).palette.panel).unwrap();
    let snapshot = gtk::Snapshot::new();
    preview.snapshot(
        &snapshot,
        source_row.width() as f64,
        source_row.height() as f64,
    );
    let node = snapshot
        .to_node()
        .expect("drag preview contains the row image");
    w.window
        .renderer()
        .unwrap()
        .render_texture(&node, None)
        .save_to_png(format!("{dir}/10-drag-preview.png"))
        .unwrap();
    let controllers = source_row.observe_controllers();
    let drag = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::DragSource>())
        .unwrap();
    assert!(
        drag.emit_by_name::<Option<gdk::ContentProvider>>("prepare", &[&80f64, &18f64])
            .is_some()
    );
    let target = row(inner);
    let controllers = target.observe_controllers();
    let drop = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i).and_downcast::<gtk::DropTarget>())
        .find(|drop| {
            drop.formats()
                .is_some_and(|formats| formats.contains_type(String::static_type()))
        })
        .unwrap();
    let y = target.height() as f64 / 2.;
    drop.emit_by_name::<gdk::DragAction>("enter", &[&80f64, &y]);
    pump(100);
    assert!(target.has_css_class("layer-drop-into"));
    capture_reference(&w, &format!("{dir}/07-group-drop-target.png"), 1.);
    assert!(drop.emit_by_name::<bool>(
        "drop",
        &[
            &glib::BoxedValue(format!("capy-layer:{texture}").to_value()),
            &80f64,
            &y
        ]
    ));
    pump(150);
    assert!(
        !state(&w)
            .layers
            .iter()
            .find(|l| l.id == inner)
            .unwrap()
            .collapsed
    );
    send(
        &w,
        A::Select {
            id: texture,
            mask: true,
        },
    );
    // Both fixed columns stay aligned at every nesting depth.
    let outer_row = row(outer);
    assert!(outer_row.width() > 0);
    let x = outer_row
        .first_child()
        .unwrap()
        .compute_bounds(&w.layer_panel.root)
        .unwrap()
        .x();
    for id in [texture, inner, outer] {
        assert_eq!(
            row(id)
                .first_child()
                .unwrap()
                .compute_bounds(&w.layer_panel.root)
                .unwrap()
                .x(),
            x
        );
    }
    let skin = state(&w)
        .layers
        .iter()
        .find(|l| l.label == "Skin base")
        .unwrap()
        .id;
    for id in [1, skin] {
        click(
            &row(id)
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Button>()
                .unwrap(),
        );
    }
    send(&w, A::ReferenceSelection);
    click(
        &row(texture)
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap(),
    );
    let current = state(&w);
    assert!(
        current
            .layers
            .iter()
            .any(|l| l.id == texture && l.editing && l.mask_selected && !l.selected)
    );
    assert_eq!(current.layers.iter().filter(|l| l.reference).count(), 3);
    send(
        &w,
        A::Lock {
            id: skin,
            value: true,
        },
    );
    edit_number(&w.layer_panel.opacity, "25*2");
    pump(100);
    assert_eq!(
        state(&w)
            .layers
            .iter()
            .find(|l| l.id == texture)
            .unwrap()
            .opacity,
        0.5
    );
    assert_eq!(
        state(&w)
            .layers
            .iter()
            .find(|l| l.id == skin)
            .unwrap()
            .opacity,
        1.
    );
    edit_number(&w.layer_panel.opacity, "100");
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    pump(300);
    assert!(
        w.layer_panel
            .root
            .measure(gtk::Orientation::Horizontal, -1)
            .0
            <= layer_ui::LAYERS_MIN_WIDTH as i32
    );
    capture_reference(&w, &format!("{dir}/08-groups-selection-dark.png"), 1.);
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Light),
    });
    pump(300);
    capture_reference(&w, &format!("{dir}/09-groups-selection-light.png"), 1.);
    send(&w, A::Collapse { id: outer });
    assert!(!state(&w).layers.iter().any(|l| l.editing));
    assert_eq!(
        state(&w).layer_tools.editing_layer.as_ref().unwrap().id,
        texture
    );
    edit_number(&w.layer_panel.opacity, "50");
    pump(100);
    assert_eq!(
        state(&w)
            .layer_tools
            .editing_layer
            .as_ref()
            .unwrap()
            .opacity,
        0.5
    );
    edit_number(&w.layer_panel.opacity, "100");
    send(&w, A::Collapse { id: outer });
    // Menus are the same shared commands as direct controls. Exercise their
    // native activation, not just the underlying Rust enum.
    let more: gtk::Button = w
        .layer_panel
        .footer
        .last_child()
        .unwrap()
        .downcast()
        .unwrap();
    let popover: gtk::PopoverMenu =
        std::iter::successors(w.layer_panel.root.first_child(), |c| c.next_sibling())
            .find_map(|c| c.downcast().ok())
            .unwrap();
    let open_menu = |id, mask, name: &str| {
        send(&w, A::Context { id, mask });
        click(&more);
        pump(120);
        capture_reference(&w, &format!("{dir}/{name}.png"), 1.);
        capture_popover(popover.upcast_ref(), &format!("{dir}/{name}-menu.png"));
    };
    open_menu(texture, true, "11-mask-context");
    let activate = |label| {
        let action = menu_action(&popover.menu_model().unwrap(), label).unwrap();
        popover.activate_action(&action, None).unwrap();
        popover.popdown();
        pump(150);
        assert!(!w.status.is_visible(), "{}", w.status.text());
    };
    activate("Copy mask");
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    open_menu(texture, false, "12-layer-context");
    activate("Rename layer…");
    let name: gtk::Entry = find_css(&row(texture), "layer-name-entry")
        .unwrap()
        .downcast()
        .unwrap();
    assert!(name.is_mapped());
    name.set_text("Fabric · copied silhouette");
    name.emit_activate();
    pump(150);
    // Clear an imported texture (no brush strokes) must invalidate its GPU
    // source and thumbnail. Undo restores the exact pixels and attached mask.
    let pixels = || {
        let button = find_css(&row(texture), "layer-thumbnail").unwrap();

        let t: gdk::Texture = descendant::<gtk::Picture>(&button)
            .unwrap()
            .paintable()
            .unwrap()
            .downcast()
            .unwrap();
        let mut bytes = vec![0; t.width() as usize * t.height() as usize * 4];
        t.download(&mut bytes, t.width() as usize * 4);
        bytes
    };
    pump(250);
    let before = pixels();
    send(&w, A::Clear { id: texture });
    pump(250);
    assert!(
        pixels() != before,
        "Clear must update imported-image GPU thumbnails"
    );
    capture_reference(&w, &format!("{dir}/13-cleared-texture.png"), 1.);
    click(&command(&w, CommandId::Undo));
    pump(250);
    assert!(pixels() == before, "Undo restores exact imported pixels");
    send(
        &w,
        A::New {
            group: false,
            clipped: false,
        },
    );
    let copied = state(&w).layer_tools.editing_layer.unwrap().id;
    send(&w, A::PasteMask { id: copied });
    send(
        &w,
        A::Select {
            id: copied,
            mask: false,
        },
    );
    w.dispatch(UiAction::SetColor {
        rgba: [0.9, 0.2, 0.5, 1.],
    });
    polygon(
        &w,
        &[[0., 0.], [2048., 0.], [2048., 1536.], [0., 1536.]],
        T::Select,
    );
    send(&w, A::FillSelection);
    send(&w, A::Deselect);
    send(
        &w,
        A::Rename {
            id: copied,
            name: "Copied mask · independent layer".into(),
        },
    );
    capture_reference(&w, &format!("{dir}/14-copied-mask.png"), 1.);
    send(&w, A::ToggleSelection { id: texture });
    click(&more);
    capture_popover(
        popover.upcast_ref(),
        &format!("{dir}/15-multi-layer-menu.png"),
    );
    activate("Group selected layers");
    let grouped = state(&w).layers.iter().find(|l| l.selected).unwrap().id;
    open_menu(grouped, false, "16-group-context");
    activate("Ungroup");
    assert!(!state(&w).layers.iter().any(|l| l.id == grouped));
    // The opacity track keeps its bounds for every digit count and text edit.
    let header = w.layer_panel.opacity.first_child().unwrap();
    let slider: gtk::Scale = header.first_child().unwrap().downcast().unwrap();
    let mut width = None;
    for value in [1., 9., 10., 99., 100.] {
        w.dispatch(UiAction::SetLayerOpacity {
            id: None,
            opacity: value / 100.,
        });
        pump(60);
        let now = slider.compute_bounds(&header).unwrap();
        if let Some(previous) = width {
            assert_eq!(now.width(), previous);
        }
        width = Some(now.width());
    }
    let value_stack: gtk::Stack = header.last_child().unwrap().downcast().unwrap();
    click(&value_stack.visible_child().unwrap().downcast().unwrap());
    pump(60);
    let entry: gtk::Entry = value_stack.visible_child().unwrap().downcast().unwrap();
    entry.set_text("50*2");
    pump(60);
    assert_eq!(
        slider.compute_bounds(&header).unwrap().width(),
        width.unwrap()
    );
    entry.emit_activate();
    pump(60);
    assert_eq!(
        slider.compute_bounds(&header).unwrap().width(),
        width.unwrap()
    );
    // Async preview arrival cannot change the thumbnail's geometry.
    let thumbnail = find_css(&row(texture), "layer-thumbnail").unwrap();
    let overlay: gtk::Overlay = thumbnail
        .clone()
        .downcast::<gtk::Button>()
        .unwrap()
        .child()
        .unwrap()
        .downcast()
        .unwrap();
    let image: gtk::Picture = overlay
        .child()
        .unwrap()
        .next_sibling()
        .unwrap()
        .downcast()
        .unwrap();
    let paintable = image.paintable();
    let before = thumbnail.compute_bounds(&row(texture)).unwrap();
    image.set_paintable(None::<&gdk::Paintable>);
    pump(30);
    let empty = thumbnail.compute_bounds(&row(texture)).unwrap();
    image.set_paintable(paintable.as_ref());
    pump(30);
    let loaded = thumbnail.compute_bounds(&row(texture)).unwrap();
    assert_eq!(
        (before.width(), before.height()),
        (empty.width(), empty.height())
    );
    assert_eq!(
        (before.width(), before.height()),
        (loaded.width(), loaded.height())
    );
    assert_eq!(loaded.width(), loaded.height());
    let paper = row(2);
    click(
        &find_css(&paper, "layer-thumbnail")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert_eq!(state(&w).layer_tools.editing_layer.unwrap().id, 2);
    assert!(
        state(&w)
            .layers
            .iter()
            .find(|l| l.id == 2)
            .unwrap()
            .selected
    );
    assert!(w.layer_panel.opacity.is_sensitive());
    let controls = state(&w).layer_tools.controls;
    assert!(controls.opacity && controls.blend && controls.mask);
    assert!(!controls.alpha_lock && !controls.fill);
    let thumbnail = find_css(&paper, "layer-thumbnail")
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    assert_eq!(
        thumbnail.tooltip_text().as_deref(),
        Some("Edit layer content")
    );
    capture_reference(&w, &format!("{dir}/17-paper-selected.png"), 1.);
    open_menu(2, false, "18-paper-context");
    popover.popdown();
    for _ in 0..110 {
        send(
            &w,
            A::New {
                group: false,
                clipped: false,
            },
        );
    }
    pump(300);
    assert!(state(&w).layers.len() > 100);
    capture_reference(&w, &format!("{dir}/05-many-layers.png"), 1.);
    w.window.close();
    pump(100);
}

fn command(w: &Workspace, id: CommandId) -> gtk::Button {
    if let Some((_, button)) = w.commands.borrow().iter().find(|(c, _)| *c == id) {
        return button.clone();
    }
    for panel in &state(w).workspace.layout.panels {
        if let Some(tile) = panel
            .tiles()
            .iter()
            .find(|t| t.control.action() == Some(UiAction::Invoke { command: id }))
            && let Some(button) =
                find_named(&w.panel_widget(panel.id), &format!("tile-{}", tile.id))
        {
            return button.downcast().unwrap();
        }
    }
    // Menu commands are native GActions, not ad-hoc GtkButtons.
    let popups: Vec<_> = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .filter_map(|p| p.downcast::<gtk::PopoverMenu>().ok())
        .filter(|p| {
            p.parent()
                .is_some_and(|p| p.is::<gtk::MenuButton>() && p.native().is_some())
        })
        .collect();
    for popup in popups {
        popup.popup();
        pump(30);
        let action = popup
            .menu_model()
            .and_then(|model| menu_action(&model, &id.label()));
        popup.popdown();
        if let Some(action) = action {
            let button = gtk::Button::new();
            button.set_sensitive(ui_session(&w).command(id).enabled);
            button.connect_clicked(move |_| {
                popup.activate_action(&action, None).unwrap();
            });
            return button;
        }
    }
    panic!("No native control for {id:?}");
}
fn double_press_handle(w: &Rc<Workspace>, point: [f32; 2]) -> bool {
    let Some(action) = w.handle_double_press_action(point) else {
        return false;
    };
    w.workspace_drag_input(ContactPhase::Cancel, point, None);
    w.dispatch(action);
    true
}

fn click(button: &gtk::Button) {
    assert!(button.is_sensitive());
    button.emit_clicked();
    pump(100);
}
fn edit_number(control: &crate::number_control::NumberControl, text: &str) {
    if let Some(spin) = control
        .first_child()
        .and_then(|header| header.last_child())
        .and_downcast::<gtk::SpinButton>()
    {
        spin.set_text(text);
        spin.update();
        pump(100);
        return;
    }
    let display: gtk::Button = find_css(control.upcast_ref(), "number-value")
        .unwrap()
        .downcast()
        .unwrap();
    click(&display);
    let entry: gtk::Entry = find_css(control.upcast_ref(), "number-entry")
        .unwrap()
        .downcast()
        .unwrap();
    entry.set_text(text);
    entry.emit_activate();
}
struct WorkspaceDragTest {
    workspace: Rc<Workspace>,
    origin: [f32; 2],
}
impl WorkspaceDragTest {
    fn update(&self, delta: [f64; 2]) {
        self.workspace.workspace_drag_input(
            ContactPhase::Move,
            [
                self.origin[0] + delta[0] as f32,
                self.origin[1] + delta[1] as f32,
            ],
            None,
        );
    }
    fn end(&self) {
        let point = self
            .workspace
            .workspace_drag
            .borrow()
            .as_ref()
            .unwrap()
            .point;
        self.workspace
            .workspace_drag_input(ContactPhase::Up, point, None);
    }
}
fn begin_workspace_drag(
    w: &Rc<Workspace>,
    widget: &gtk::Widget,
    x: f32,
    y: f32,
) -> WorkspaceDragTest {
    let point = widget
        .compute_point(&w.surface, &gtk::graphene::Point::new(x, y))
        .unwrap();
    assert!(
        w.drag_target_at([point.x(), point.y()]).is_some(),
        "No drag target at {}, {} (picked {:?}, expected {:?})",
        point.x(),
        point.y(),
        w.surface
            .pick(point.x() as f64, point.y() as f64, gtk::PickFlags::DEFAULT),
        widget
    );
    let controllers = w.surface.observe_controllers();
    let _controller = (0..controllers.n_items())
        .filter_map(|i| {
            controllers
                .item(i)
                .and_downcast::<gtk::EventControllerLegacy>()
        })
        .find(|g| g.name().as_deref() == Some("workspace-drag"))
        .unwrap();
    let origin = [point.x(), point.y()];
    w.workspace_drag_input(ContactPhase::Down, origin, None);
    WorkspaceDragTest {
        workspace: w.clone(),
        origin,
    }
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_toolbar_sizing() {
    let app = native_test_app("dev.layer.ToolbarSizingTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let dir = artifact_dir("../../artifacts/ui/workspace-management/gtk");
    let initial = state(&w).workspace;
    let placement = || {
        w.resolved()
            .groups
            .into_iter()
            .find(|g| g.panels.contains(&Panel::Toolbar))
            .unwrap()
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(initial.clone()),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Toolbar,
            viewport,
            target: DockTarget::Float {
                position: [600.0, 300.0],
            },
        });
        pump(120);
        for mode in [
            FloatingToolbarLayout::Vertical,
            FloatingToolbarLayout::Horizontal,
            FloatingToolbarLayout::Compact,
        ] {
            w.dispatch(UiAction::DoubleClickPanelHandle { group: placement().id, viewport });
            pump(250);
            assert_eq!(state(&w).workspace.layout.floating[0].toolbar_layout, mode);
            let strip = w.panel_widget(Panel::Toolbar);
            let handle = find_css(&strip, "panel-grip").unwrap();
            let actual = handle.compute_bounds(&strip).unwrap();
            if mode == FloatingToolbarLayout::Horizontal {
                assert_eq!(actual.x() + actual.width(), strip.width() as f32);
            } else {
                assert_eq!(actual.y() + actual.height(), strip.height() as f32);
            }
        }
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            w.dispatch(UiAction::MovePanel {
                panel: Panel::Toolbar,
                viewport,
                target: DockTarget::Edge { edge, outer: true },
            });
            pump(100);
            let g = placement();
            assert!(!g.floating);
            // Lane count depends on the current toolbar contents, style and
            // viewport. Verify the native allocations against the shared wrap
            // projection instead of assuming a one-lane ribbon.
            let strip = w.panel_widget(Panel::Toolbar);
            let projection = g.tiles.as_ref().unwrap();
            let mut child = strip.first_child();
            for expected in &projection.tiles {
                let tile = child.take().unwrap();
                child = tile.next_sibling();
                let actual = tile.compute_bounds(&strip).unwrap();
                for (actual, expected) in [
                    (actual.x(), expected.x),
                    (actual.y(), expected.y),
                    (actual.width(), expected.width),
                    (actual.height(), expected.height),
                ] {
                    assert!(
                        (actual - expected).abs() <= 1.,
                        "native toolbar follows shared wrapping"
                    );
                }
            }
            capture_reference(
                &w,
                &format!("{dir}/toolbar-docked-{edge:?}-{theme:?}.png"),
                1.0,
            );
        }
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_zen_icons() {
    let app = native_test_app("dev.layer.ZenIconsTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::Edit {
            id: PreferenceId::ZenRevealAtEdges,
            value: PreferenceValue::Bool(true),
        },
    });
    let dir = artifact_dir("../../artifacts/ui/zen-icons");
    let zen = command(&w, CommandId::ZenMode);
    let image = zen.child().and_downcast::<gtk::Image>().unwrap();
    let bounds = zen.compute_bounds(&w.surface).unwrap();
    assert_eq!(
        image.pixel_size(),
        (state(&w).workspace.layout.header.size.tile() * 440. / 512.).round() as i32
    );
    assert_eq!(w.preferences.dialog.content_height(), 744);
    assert_eq!(
        crate::icons::name(&image).as_deref(),
        Some("layer-zen-looking-up-symbolic")
    );
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::OpenSettings {
            page: SettingsPage::Appearance,
        });
        pump(300);
        let row = find_named(w.preferences.dialog.upcast_ref(), "setting-zen-icon").unwrap();
        let grid = find_css(&row, "image-selector")
            .unwrap()
            .downcast::<gtk::Grid>()
            .unwrap();
        let scroll = grid
            .ancestor(gtk::ScrolledWindow::static_type())
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        let adjustment = scroll.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
        pump(200);
        let tile_bounds = grid.compute_bounds(&row).unwrap();
        assert!(
            (tile_bounds.x() + tile_bounds.width() / 2.0 - row.width() as f32 / 2.0).abs() < 1.0
        );
        for (i, (symbol, label)) in layer_ui::ZenIcon::CHOICES.into_iter().enumerate() {
            let tile = grid
                .child_at(i as i32, 0)
                .unwrap()
                .downcast::<gtk::ToggleButton>()
                .unwrap();
            assert_eq!(tile.tooltip_text().as_deref(), Some(label));
            tile.emit_clicked();
            pump(100);
            assert!(tile.is_active());
            assert_eq!(state(&w).settings.zen_icon, symbol);
            assert_eq!(
                crate::icons::name(&image).as_deref(),
                Some(format!("layer-{}-symbolic", symbol.icon()).as_str())
            );
            assert_eq!(
                image.pixel_size(),
                (state(&w).workspace.layout.header.size.tile() * 440. / 512.).round() as i32
            );
            assert_eq!(zen.compute_bounds(&w.surface).unwrap(), bounds);
            for other in 0..4 {
                assert_eq!(
                    grid.child_at(other, 0)
                        .unwrap()
                        .downcast::<gtk::ToggleButton>()
                        .unwrap()
                        .is_active(),
                    other as usize == i
                );
            }
            capture_reference(&w, &format!("{dir}/gtk-selector-{name}-{i}.png"), 1.0);
        }
        // Reset uses the same action as the existing per-setting context menu.
        w.dispatch(UiAction::Preferences {
            action: PreferenceAction::Reset {
                id: PreferenceId::ZenIcon,
            },
        });
        pump(100);
        assert!(
            grid.child_at(0, 0)
                .unwrap()
                .downcast::<gtk::ToggleButton>()
                .unwrap()
                .is_active()
        );
        capture_reference(&w, &format!("{dir}/gtk-selector-{name}.png"), 1.0);
        w.dispatch(UiAction::CloseSettings);
        pump(250);
        // Total Zen hides the button with the header, retaining its selected
        // state for the edge reveal.
        click(&zen);
        w.chrome_event(layer_ui::ChromeEvent::Motion {
            position: [600.0, 450.0],
        });
        pump(250);
        assert!(zen.has_css_class("selected-tool"));
        assert!(!w.header.root.can_target());
        capture_reference(&w, &format!("{dir}/gtk-total-zen-{name}.png"), 1.0);
        w.chrome_event(layer_ui::ChromeEvent::Motion {
            position: [600.0, 1.0],
        });
        pump(250);
        assert!(w.header.root.can_target());
        capture_reference(&w, &format!("{dir}/gtk-active-{name}.png"), 1.0);
        click(&zen);
        pump(250);
        assert!(!zen.has_css_class("selected-tool"));
        capture_reference(&w, &format!("{dir}/gtk-inactive-{name}.png"), 1.0);
    }
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "group tab presentation: requires private Wayland and GPU"]
fn native_group_tab_styles() {
    let app = native_test_app("art.capycanvas.GroupTabStylesTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(500);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let group = state(&w)
        .workspace
        .layout
        .panel_group(Panel::Brushes)
        .unwrap();
    for panel in [Panel::Sizes, Panel::Layers] {
        w.dispatch(UiAction::MovePanel {
            panel,
            viewport,
            target: DockTarget::Tab { group, index: None },
        });
    }
    let dir = artifact_dir("../../artifacts/ui/group-tab-styles");
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for style in TabStyle::ALL {
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetTabStyle { group, style },
            });
            for active in [Panel::Brushes, Panel::Sizes, Panel::Layers] {
                w.dispatch(UiAction::SelectPanelTab {
                    group,
                    panel: active,
                });
                pump(150);
                let layout = state(&w).workspace.layout;
                for (panel, button) in &w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == group)
                    .unwrap()
                    .tabs
                {
                    let content = button.child().unwrap();
                    let icon = content.first_child().and_downcast::<gtk::Image>().unwrap();
                    let label = content.last_child().and_downcast::<gtk::Label>().unwrap();
                    let expected = layout.tab_presentation(*panel);
                    assert_eq!(icon.is_visible(), expected.show_icon);
                    if style != TabStyle::Automatic {
                        assert_eq!(label.is_visible(), expected.show_name);
                    }
                    if !label.is_visible() {
                        let bounds = button.compute_bounds(&w.surface).unwrap();
                        assert_eq!(bounds.width(), bounds.height(), "icon-only tabs are square");
                    }
                    assert_eq!(label.text(), layout.panel(*panel).unwrap().canonical_title());
                    assert_eq!(
                        button.compute_bounds(&w.surface).unwrap().height(),
                        TAB_BAR_HEIGHT
                    );
                }
            }
            capture_reference(&w, &format!("{dir}/gtk-{theme:?}-{style:?}.png"), 1.0);
        }
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetTabStyle {
                group,
                style: TabStyle::Automatic,
            },
        });
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Layers,
            viewport,
            target: DockTarget::Float {
                position: [850.0, 200.0],
            },
        });
        pump(150);
        let visible_names = || {
            w.groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .tabs
                .iter()
                .filter(|(_, button)| button.child().unwrap().last_child().unwrap().is_visible())
                .count()
        };
        assert_eq!(visible_names(), 2);
        capture_reference(
            &w,
            &format!("{dir}/gtk-{theme:?}-Automatic-two-tabs.png"),
            1.0,
        );
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Layers,
            viewport,
            target: DockTarget::Tab { group, index: None },
        });
        pump(150);
        assert_eq!(
            visible_names(),
            3,
            "a group fitted to its tabs has room for every name"
        );
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "native settings typography/geometry reference: requires private Wayland and GPU"]
fn native_settings_typography() {
    // Measure an unmodified Adwaita row first, before loading application CSS.
    adw::init().unwrap();
    let reference = adw::Window::new();
    let css = gtk::CssProvider::new();
    css.load_from_string(&format!("window {{ font-size: {UI_TEXT_PT}pt; }}"));
    let display = gdk::Display::default().unwrap();
    gtk::style_context_add_provider_for_display(
        &display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let row = adw::ActionRow::builder()
        .title("Native title")
        .subtitle("Native description")
        .build();
    let list = gtk::ListBox::new();
    list.append(&row);
    reference.set_content(Some(&list));
    reference.present();
    pump(150);
    let font_px = |widget: &gtk::Widget| {
        widget.pango_context().font_description().unwrap().size() as f64 / gtk::pango::SCALE as f64
    };
    let title_px = font_px(&find_css(row.upcast_ref(), "title").unwrap());
    let subtitle_px = font_px(&find_css(row.upcast_ref(), "subtitle").unwrap());
    assert!(subtitle_px < title_px);
    eprintln!(
        "Unmodified Adwaita: title={title_px:.3}px, subtitle={subtitle_px:.3}px, ratio={:.4}",
        subtitle_px / title_px
    );
    reference.destroy();
    gtk::style_context_remove_provider_for_display(&display, &css);

    fn labels(widget: &gtk::Widget, root: &gtk::Widget, out: &mut Vec<serde_json::Value>) {
        if let Some(label) = widget.downcast_ref::<gtk::Label>()
            && widget.is_mapped()
        {
            let b = widget.compute_bounds(root).unwrap();
            out.push(serde_json::json!({"text":label.text().to_string(), "font_px":label.pango_context().font_description().unwrap().size() as f64 / gtk::pango::SCALE as f64,
                "bounds":[b.x(),b.y(),b.width(),b.height()]}));
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            labels(&c, root, out);
        }
    }
    let app = native_test_app("art.capycanvas.SettingsTypographyTest");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = fixture_workspace(&app);
    w.window.present();
    pump(600);
    let dir = artifact_dir("../../artifacts/ui/settings-audit");
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for page in SettingsPage::ALL {
            w.dispatch(UiAction::OpenSettings { page });
            pump(200);
            let content =
                find_named(w.preferences.dialog.upcast_ref(), "preferences-content").unwrap();
            let view = ui_session(&w)
                .preferences()
                .unwrap();
            let page = view.pages.iter().find(|p| p.id == page).unwrap();
            let mut rows = Vec::new();
            for row in page.groups.iter().flat_map(|g| &g.rows) {
                let widget = find_named(
                    w.preferences.dialog.upcast_ref(),
                    &format!("preference-{}", row.id.key()),
                )
                .or_else(|| {
                    find_named(
                        w.preferences.dialog.upcast_ref(),
                        &format!("setting-{}", row.id.key()),
                    )
                })
                .unwrap();
                let b = widget.compute_bounds(&content).unwrap();
                let mut text = Vec::new();
                labels(&widget, &content, &mut text);
                for (expected, size) in [(&row.title, title_px), (&row.description, subtitle_px)] {
                    if expected.is_empty() {
                        continue;
                    }
                    let label = text
                        .iter()
                        .find(|l| l["text"] == *expected)
                        .unwrap_or_else(|| panic!("Missing settings label: {expected}"));
                    assert!(
                        (label["font_px"].as_f64().unwrap() - size).abs() < 0.02,
                        "{expected}: {label}"
                    );
                }
                rows.push(serde_json::json!({"id":row.id,"bounds":[b.x(),b.y(),b.width(),b.height()],"labels":text}));
            }
            if page.id == SettingsPage::Shortcuts {
                for id in std::iter::once("shortcuts-search".into())
                    .chain(view.shortcuts.iter().map(|s| format!("shortcut-{}", s.id)))
                {
                    let Some(widget) = find_named(w.preferences.dialog.upcast_ref(), &id) else {
                        continue;
                    };
                    let Some(b) = widget.compute_bounds(&content).filter(|_| widget.is_mapped()) else {
                        continue;
                    };
                    let mut text = Vec::new();
                    labels(&widget, &content, &mut text);
                    rows.push(serde_json::json!({"id":id,"bounds":[b.x(),b.y(),b.width(),b.height()],"labels":text}));
                }
            }
            capture_reference(&w, &format!("{dir}/gtk-{}-{name}.png", page.id.key()), 1.0);
            std::fs::write(format!("{dir}/gtk-{}-{name}.json", page.id.key()), serde_json::to_vec_pretty(&serde_json::json!({
                "title_px":title_px,"subtitle_px":subtitle_px,"content_width":content.width(),"rows":rows})).unwrap()).unwrap();
        }
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_zen_behaviors() {
    let app = native_test_app("art.capycanvas.ZenTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let zen = command(&w, CommandId::ZenMode);
    let saved = state(&w).workspace.layout;
    for theme in [Theme::Dark, Theme::Light] {
        for show in [true, false] {
            for edges in [false, true] {
                w.dispatch(UiAction::SetTheme { theme: Some(theme) });
                for (id, value) in [
                    (PreferenceId::ZenShowCapy, show),
                    (PreferenceId::ZenRevealAtEdges, edges),
                ] {
                    w.dispatch(UiAction::Preferences {
                        action: PreferenceAction::Edit {
                            id,
                            value: PreferenceValue::Bool(value),
                        },
                    });
                }
                click(&zen);
                let reply = w.chrome_event(ChromeEvent::Motion {
                    position: [600., 450.],
                });
                assert!(reply.chrome_hidden);
                assert_eq!(w.zen_capy.is_visible(), show);
                assert!(!w.header.root.can_target());
                for (slot, widget) in w.surface.imp().children.borrow().iter() {
                    if !matches!(slot, Slot::Canvas | Slot::CanvasBar) && !widget.has_css_class("floating-panel") {
                        assert!(widget.has_css_class("zen-hidden") && !widget.can_target());
                    }
                }
                assert_eq!(w.reveal_chrome_at(600., 6.), edges);
                assert_eq!(w.header.root.can_target(), edges);
                if show && !edges {
                    click(&w.zen_capy);
                } else {
                    for pressed in [true, false] {
                        w.interact(UiInput::Key {
                            key: "Tab".into(),
                            pressed,
                            repeat: false,
                            modifiers: Modifiers::default(),
                            editing: false,
                            divider: None,
                        });
                    }
                }
                assert!(!state(&w).workspace.zen_mode);
                assert!(!w.zen_capy.is_visible());
                assert_eq!(state(&w).workspace.layout, saved);
            }
        }
    }
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Appearance,
    });
    pump(250);
    assert!(find_named(w.preferences.dialog.upcast_ref(), "setting-zen-icon").is_some());
    assert!(find_named(w.preferences.dialog.upcast_ref(), "setting-zen-show-capy").is_some());
    assert!(
        find_named(
            w.preferences.dialog.upcast_ref(),
            "setting-zen-reveal-at-edges"
        )
        .is_some()
    );
    w.dispatch(UiAction::CloseSettings);
    pump(250);
    w.window.close();
    pump(100);
    // Match application shutdown: a cold shader worker can still be using the
    // graphics driver after the window closes and before this test process exits.
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_same_slot_drop() {
    let app = native_test_app("art.capycanvas.SameSlotDrop");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    for tearoff in [false, true] {
        let root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == 5)
            .unwrap()
            .root
            .clone();
        let before = root.compute_bounds(&w.surface).unwrap();
        let mut baseline = state(&w).workspace;
        baseline.layout.measurements.clear();
        let grip = find_css(root.upcast_ref(), "panel-grip").unwrap();
        let origin = grip
            .compute_point(&w.surface, &gtk::graphene::Point::new(3.0, 3.0))
            .unwrap();
        let drag = begin_workspace_drag(&w, &grip, 3.0, 3.0);
        if tearoff {
            drag.update([(600.0 - origin.x()) as f64, (400.0 - origin.y()) as f64]);
            pump(120);
            assert_eq!(state(&w).workspace.layout.floating.len(), 1);
        }
        let neighbor = w
            .resolved()
            .groups
            .into_iter()
            .find(|g| g.id == 6)
            .unwrap()
            .bounds;
        drag.update([
            (neighbor.x + neighbor.width * 0.5 - origin.x()) as f64,
            (HEADER_HEIGHT * 0.5 - origin.y()) as f64,
        ]);
        pump(80);
        drag.end();
        pump(350);
        let mut after = state(&w).workspace;
        after.layout.measurements.clear();
        assert_eq!(after, baseline, "tearoff={tearoff}");
        let root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == 5)
            .unwrap()
            .root
            .clone();
        let after = root.compute_bounds(&w.surface).unwrap();
        assert_eq!(
            [after.x(), after.y(), after.width(), after.height()],
            [before.x(), before.y(), before.width(), before.height()]
        );
        assert!(!w.status.is_visible(), "{}", w.status.text());
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_floating_gestures() {
    let app = native_test_app("dev.layer.FloatingGesturesTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Sizes,
        viewport,
        target: DockTarget::Float {
            position: [640.0, 250.0],
        },
    });
    // This regression exercises an explicitly visible floating title bar.
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetTabHidden {
            panel: Panel::Sizes,
            hidden: false,
        },
    });
    pump(150);
    let group = state(&w)
        .workspace
        .layout
        .panel_group(Panel::Sizes)
        .unwrap();
    let baseline = state(&w).workspace;
    let restore = || {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(baseline.clone()),
        });
        pump(120);
    };
    let bounds = || {
        w.resolved()
            .groups
            .into_iter()
            .find(|g| g.id == group)
            .unwrap()
            .bounds
    };
    let root = || {
        w.groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .root
            .clone()
    };
    let dir = artifact_dir("../../artifacts/ui/workspace-management/gtk");
    let initial = bounds();
    let handles = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.id == group)
        .unwrap()
        .resize_handles;
    for handle in handles {
        restore();
        let b = handle.bounds;
        let native = find_named(
            w.surface.upcast_ref(),
            &format!("floating-resize-{group}-{:?}", handle.edge),
        )
        .unwrap();
        let point = [b.x + b.width / 2.0, b.y + b.height / 2.0];
        assert!(
            matches!(w.drag_target_at(point), Some(DragTarget::Resize(id, edge)) if id == group && edge == handle.edge)
        );
        assert!(!initial.contains(point[0], point[1]));
        let drag = begin_workspace_drag(&w, &native, b.width / 2.0, b.height / 2.0);
        drag.update([18.0f64, 16.0f64]);
        pump(40);
        assert_ne!(bounds(), initial);
        drag.end();
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        // Undo restores durable state; native text measurements may still be
        // settling after the resize and are intentionally not history entries.
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(&baseline).unwrap()
        );
    }
    // Releasing into each screen edge rebuilds the docking widgets while the
    // workspace gesture remains alive. This caught the RefCell teardown panic.
    for (edge, target) in [
        (Edge::Left, [1.0, viewport[1] * 0.5]),
        (Edge::Right, [viewport[0] - 1.0, viewport[1] * 0.5]),
    ] {
        restore();
        let tab = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .tabs[0]
            .1
            .clone();
        let origin = tab
            .compute_point(&w.surface, &gtk::graphene::Point::new(12.0, 12.0))
            .unwrap();
        let drag = begin_workspace_drag(&w, tab.upcast_ref(), 12.0, 12.0);
        let delta = [
            (target[0] - origin.x()) as f64,
            (target[1] - origin.y()) as f64,
        ];
        drag.update([delta[0], delta[1]]);
        pump(60);
        let hint = w
            .drop_hint
            .borrow()
            .clone()
            .expect("screen edge has a snap line");
        assert_eq!(hint.target, DockTarget::Edge { edge, outer: true });
        if edge == Edge::Top {
            assert_eq!(hint.bounds.y, HEADER_HEIGHT);
        }
        capture_reference(&w, &format!("{dir}/snap-{edge:?}.png"), 1.0);
        drag.end();
        pump(100);
        assert!(state(&w).workspace.layout.floating.is_empty());
        assert_eq!(
            state(&w).workspace.layout.panel_group(Panel::Sizes),
            Some(group)
        );
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(&baseline).unwrap()
        );
    }
    restore();
    // Create space in the title bar and verify inside-top is move, not resize.
    let corner = [
        initial.x + initial.width + 2.0,
        initial.y + initial.height + 2.0,
    ];
    for (phase, position) in [
        (ContactPhase::Down, corner),
        (ContactPhase::Up, [corner[0] + 160.0, corner[1] + 80.0]),
    ] {
        w.dispatch(UiAction::ResizeFloating {
            group,
            edge: ResizeEdge::BottomRight,
            phase,
            position,
            viewport,
        });
    }
    pump(80);
    let before = bounds();
    let header = find_css(root().upcast_ref(), "dock-tabs").unwrap();
    let point = header
        .compute_point(
            &w.surface,
            &gtk::graphene::Point::new(header.width() as f32 - 28.0, 1.0),
        )
        .unwrap();
    assert!(
        matches!(w.drag_target_at([point.x(), point.y()]), Some(DragTarget::Dock(DockItem::Group { group: id })) if id == group)
    );
    // Tabs are excluded from double-click reset.
    assert!(!double_press_handle(&w, [before.x + 15.0, before.y + 15.0]));
    assert_eq!(bounds(), before);
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(true);
    assert!(double_press_handle(&w, [point.x(), point.y()]));
    assert_eq!(
        [bounds().width, bounds().height],
        [initial.width, initial.height]
    );
    pump(50);
    let middle = root().compute_bounds(&w.surface).unwrap();
    assert!(
        middle.width() > initial.width && middle.width() < before.width,
        "reset interpolates: {} < {} < {}",
        initial.width,
        middle.width(),
        before.width
    );
    capture_reference(&w, &format!("{dir}/reset-size-mid-animation.png"), 1.0);
    pump(220);
    assert_eq!(root().width() as f32, initial.width);
    capture_reference(&w, &format!("{dir}/reset-size-complete.png"), 1.0);
    w.dispatch(UiAction::Invoke {
        command: CommandId::UndoWorkspace,
    });
    assert_eq!(bounds(), before);
    restore();
    // At natural size, empty title space toggles the lone panel's header.
    // The entire footer strip toggles it back, including outside the dots.
    assert!(double_press_handle(
        &w,
        [initial.x + initial.width - 28.0, initial.y + 10.0]
    ));
    pump(250);
    assert!(
        state(&w)
            .workspace
            .layout
            .panel(Panel::Sizes)
            .unwrap()
            .hide_tab
    );
    let g = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.id == group)
        .unwrap();
    let footer = g.footer_grip.unwrap();
    let point = [g.bounds.x + footer.x + 3.0, g.bounds.y + footer.y + 10.0];
    assert!(
        matches!(w.drag_target_at(point), Some(DragTarget::Dock(DockItem::Group { group: id })) if id == group)
    );
    capture_reference(&w, &format!("{dir}/panel-cycle-tab-hidden.png"), 1.0);
    assert!(double_press_handle(&w, point));
    pump(250);
    assert!(
        !state(&w)
            .workspace
            .layout
            .panel(Panel::Sizes)
            .unwrap()
            .hide_tab
    );
    assert_eq!(bounds(), initial);
    capture_reference(&w, &format!("{dir}/panel-cycle-tab-shown.png"), 1.0);
    restore();
    // The wider blue line targets the entire original stacked sidebar.
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    pump(120);
    let strip = w.panel_widget(Panel::Toolbar);
    let grip = find_css(&strip, "panel-grip").unwrap();
    let origin = grip
        .compute_point(&w.surface, &gtk::graphene::Point::new(10.0, 10.0))
        .unwrap();
    let drag = begin_workspace_drag(&w, &grip, 10.0, 10.0);
    // Tearing off the default left toolbar shifts the adjacent sidebar.
    // Resolve its snap point after that move, as the pointer preview does.
    drag.update([(650. - origin.x()) as f64, (450. - origin.y()) as f64]);
    pump(80);
    let left = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.active == Panel::Brushes)
        .unwrap();
    let point = [
        left.bounds.x + left.bounds.width + 60.0,
        left.bounds.y + left.bounds.height * 0.5,
    ];
    let delta = [
        (point[0] - origin.x()) as f64,
        (point[1] - origin.y()) as f64,
    ];
    drag.update([delta[0], delta[1]]);
    pump(80);
    assert!(matches!(
        w.drop_hint.borrow().as_ref().unwrap().target,
        DockTarget::BesideBand { .. }
    ));
    capture_reference(&w, &format!("{dir}/whole-sidebar-snap.png"), 1.0);
    drag.end();
    pump(100);
    assert!(state(&w).workspace.layout.floating.is_empty());
    w.window.close();
    pump(100);
}
fn white_pixels(w: &Workspace) -> usize {
    // Inspect the same whole-window scene as the review PNG, not a fresh
    // standalone canvas snapshot that could hide host invalidation errors.
    let texture = crate::snapshot(w);
    canvas_white(w, &texture)
}
fn canvas_white(w: &Workspace, texture: &gdk::Texture) -> usize {
    let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut bytes, texture.width() as usize * 4);
    let bounds = w.area.compute_bounds(&w.window).unwrap();
    let stride = texture.width() as usize * 4;
    (bounds.y() as usize..(bounds.y() + bounds.height()) as usize)
        .map(|y| {
            let row = &bytes[y * stride + bounds.x() as usize * 4
                ..y * stride + (bounds.x() + bounds.width()) as usize * 4];
            row.chunks_exact(4)
                .filter(|p| p[0] > 245 && p[1] > 245 && p[2] > 245 && p[3] > 245)
                .count()
        })
        .sum()
}

fn capture_reference(w: &Workspace, path: &str, scale: f32) {
    // Include AdwDialog's overlay host, not only ApplicationWindow::child().
    // This measures GTK widgets, not compositor delivery or OS window shadows.
    crate::with_canvas_snapshot(w, || {
        crate::snapshot_window(&w.window, scale)
            .save_to_png(path)
            .unwrap();
    });
}

fn menu_action(model: &gtk::gio::MenuModel, label: &str) -> Option<String> {
    for i in 0..model.n_items() {
        if model
            .item_attribute_value(i, "label", None)
            .and_then(|v| v.get::<String>())
            .as_deref()
            == Some(label)
            && let Some(action) = model
                .item_attribute_value(i, "action", None)
                .and_then(|v| v.get::<String>())
        {
            return Some(action);
        }
        for link in ["section", "submenu"] {
            if let Some(child) = model.item_link(i, link)
                && let Some(action) = menu_action(&child, label)
            {
                return Some(action);
            }
        }
    }
    None
}

fn capture_popover(popover: &gtk::Popover, path: &str) {
    popover.present();
    pump(100);
    // An occluded Wayland popup can still be awaiting configure. Complete its
    // real native allocation for widget inspection, not a presentation timing test.
    let width = popover
        .width()
        .max(popover.measure(gtk::Orientation::Horizontal, -1).1);
    let height = popover
        .height()
        .max(popover.measure(gtk::Orientation::Vertical, width).1);
    popover.allocate(width, height, -1, None);
    let snapshot = gtk::Snapshot::new();
    let mut child = popover.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        popover.snapshot_child(&widget, &snapshot);
    }
    popover
        .renderer()
        .unwrap()
        .render_texture(snapshot.to_node().unwrap_or_else(|| panic!(
            "Empty popup capture {path}: visible={}, mapped={}, size={}x{}, opacity={}, child={:?}",
            popover.is_visible(), popover.is_mapped(), popover.width(), popover.height(), popover.opacity(),
            popover.first_child().map(|c| (c.is_visible(), c.is_mapped(), c.width(), c.height()))
        )), None)
        .save_to_png(path)
        .unwrap();
}

#[test]
#[ignore = "workspace management: requires a private Wayland/Vulkan display"]
fn native_workspace_management() {
    let app = native_test_app("dev.layer.WorkspaceManagementTest");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = fixture_workspace(&app);
    w.window.present();
    pump(600);
    let dir = artifact_dir("../../artifacts/ui/workspace-management/gtk");
    let initial = state(&w).workspace;
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let send = |action| {
        w.dispatch(UiAction::Customize { action });
        pump(80);
    };
    let menu = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .find(|p| p.has_css_class("panel-context-menu") && p.is::<gtk::PopoverMenu>())
        .unwrap()
        .downcast::<gtk::PopoverMenu>()
        .unwrap();
    menu.set_autohide(false);
    menu.set_pointing_to(Some(&gdk::Rectangle::new(350, 170, 1, 1)));
    let open_context = |target| {
        let model = ui_session(&w)
            .context_menu(target)
            .unwrap();
        w.populate_workspace_menu(&menu, model);
        menu.popup();
        pump(100);
    };
    let activate = |popup: &gtk::PopoverMenu, label: &str| {
        let action = menu_action(&popup.menu_model().unwrap(), label)
            .unwrap_or_else(|| panic!("Missing menu item {label}"));
        popup.activate_action(&action, None).unwrap();
        pump(150);
    };
    let workspace_menu = named::<gtk::PopoverMenu>(w.window.upcast_ref(), "workspace-menu");
    workspace_menu.set_autohide(false);
    let snapshot = |name: &str| {
        pump(120);
        capture_reference(&w, &format!("{dir}/{name}.png"), 1.0);
    };
    let prompt = || {
        named::<adw::AlertDialog>(w.window.upcast_ref(), "toolbar-dialog")
    };
    let confirm_prompt = || {
        let label = ui_session(&w)
            .toolbar_prompt()
            .unwrap()
            .confirm_label;
        click(&find_button(prompt().upcast_ref(), &label).unwrap());
        pump(180);
        assert!(
            ui_session(&w)
                .toolbar_prompt()
                .is_none()
        );
        assert!(find_named(w.window.upcast_ref(), "toolbar-dialog").is_none());
    };
    let group = |panel| state(&w).workspace.layout.panel_group(panel).unwrap();
    let placement = |panel| {
        w.resolved()
            .groups
            .into_iter()
            .find(|g| g.panels.contains(&panel))
            .unwrap()
    };
    let live = || {
        ui_session(&w)
            .workspace_update()
            .drag
            .unwrap()
            .group
            .unwrap()
            .bounds
    };

    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(initial.clone()),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(180);
        workspace_menu.popup();
        pump(120);
        capture_popover(
            workspace_menu.upcast_ref(),
            &format!("{dir}/workspace-menu-{theme:?}.png"),
        );
        activate(
            &workspace_menu,
            &format!("{} panel", Panel::Brushes.canonical_label().as_ref()),
        );
        assert!(
            state(&w)
                .workspace
                .layout
                .panel_group(Panel::Brushes)
                .is_none()
        );
        workspace_menu.popup();
        activate(
            &workspace_menu,
            &format!("{} panel", Panel::Brushes.canonical_label().as_ref()),
        );
        assert!(
            state(&w)
                .workspace
                .layout
                .panel_group(Panel::Brushes)
                .is_some()
        );
        workspace_menu.popup();
        activate(&workspace_menu, "New Toolbar…");
        assert!(find_named(w.window.upcast_ref(), "tool-picker").is_some());
        send(CustomizationAction::CancelTools);

        open_context(ContextTarget::Ribbon {
            panel: Panel::Toolbar,
        });
        capture_popover(
            menu.upcast_ref(),
            &format!("{dir}/toolbar-menu-{theme:?}.png"),
        );
        activate(&menu, "Duplicate Tools toolbar…");
        let name = named::<adw::EntryRow>(w.window.upcast_ref(), "edit-toolbar-name");
        assert_eq!(name.text(), "Tools Copy");
        name.set_text(Panel::Brushes.canonical_label().as_ref());
        assert!(!prompt().is_response_enabled("confirm"));
        name.set_text("Painting Tools");
        assert!(prompt().is_response_enabled("confirm"));
        snapshot(&format!("duplicate-{theme:?}"));
        confirm_prompt();
        let panel = state(&w)
            .workspace
            .layout
            .panels
            .iter()
            .find(|p| p.canonical_title() == "Painting Tools")
            .unwrap()
            .id;
        open_context(ContextTarget::Ribbon { panel });
        activate(&menu, "Rename Painting Tools toolbar…");
        name.set_text("Painting");
        snapshot(&format!("rename-{theme:?}"));
        confirm_prompt();
        assert_eq!(
            state(&w).workspace.layout.panel(panel).unwrap().canonical_title(),
            "Painting"
        );

        // Pull away from the source: the live float exists before release,
        // and free canvas has no target rectangle.
        let area = w.resolved().work_area;
        let point = [area.x + area.width * 0.5, area.y + area.height * 0.4];
        let item = DockItem::Panel { panel };
        let tab = find_css(&w.panel_widget(panel), "panel-grip").unwrap();
        let origin = tab
            .compute_point(&w.surface, &gtk::graphene::Point::new(10.0, 10.0))
            .unwrap();
        snapshot(&format!("before-tear-off-{theme:?}"));
        let drag = begin_workspace_drag(&w, &tab, 10.0, 10.0);
        let delta = [
            (point[0] - origin.x()) as f64,
            (point[1] - origin.y()) as f64,
        ];
        drag.update([delta[0], delta[1]]);
        pump(100);
        assert!(placement(panel).floating);
        assert!(w.drop_at(point[0], point[1], item).is_none());
        assert!(w.drop_hint.borrow().is_none());
        snapshot(&format!("tear-off-live-{theme:?}"));
        drag.end();
        pump(180);
        for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
            open_context(ContextTarget::Ribbon { panel });
            activate(&menu, style.label());
            // Recreate using each style's default floating dimensions.
            w.dispatch(UiAction::MovePanel {
                panel,
                viewport,
                target: DockTarget::Edge {
                    edge: Edge::Top,
                    outer: false,
                },
            });
            w.dispatch(UiAction::MovePanel {
                panel,
                viewport,
                target: DockTarget::Float { position: point },
            });
            pump(180);
            let p = placement(panel);
            let columns = if style == TileStyle::Labeled {
                2.0
            } else {
                3.0
            };
            assert_eq!(
                p.bounds.width,
                columns * (style.size()[0] + style.gap()) - style.gap()
            );
            let strip = w.panel_widget(panel);
            let tile = strip
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap_or_else(|| strip.first_child().unwrap());
            assert_eq!(
                [tile.width(), tile.height()],
                style.size().map(|v| v as i32)
            );
            snapshot(&format!("floating-{style:?}-{theme:?}"));
        }
        let p = placement(panel);
        let strip = w.panel_widget(panel);
        let grip = find_css(&strip, "panel-grip").unwrap();
        assert_eq!(grip.width(), strip.width());
        let drag = begin_workspace_drag(&w, &grip, 3.0, 10.0);
        drag.update([45.0f64, 30.0f64]);
        pump(100);
        assert_eq!(live().x, p.bounds.x + 45.0);
        assert_eq!(live().y, p.bounds.y + 30.0);
        snapshot(&format!("live-toolbar-move-{theme:?}"));
        drag.end();
        pump(100);
        workspace_menu.popup();
        activate(&workspace_menu, "Undo Layout Change");
        assert_eq!(placement(panel).bounds, p.bounds);
        workspace_menu.popup();
        activate(&workspace_menu, "Redo Layout Change");
        assert_eq!(placement(panel).bounds.x, p.bounds.x + 45.0);

        let floated = group(panel);
        // Narrow the float first, so adding a tab must actually grow it.
        let before = placement(panel).bounds;
        let corner = [before.x + before.width, before.y + before.height];
        for (phase, position) in [
            (ContactPhase::Down, corner),
            (ContactPhase::Up, [before.x + 112.0, corner[1]]),
        ] {
            w.dispatch(UiAction::ResizeFloating {
                group: floated,
                edge: ResizeEdge::BottomRight,
                phase,
                position,
                viewport,
            });
        }
        pump(100);
        let narrow_width = placement(panel).bounds.width;
        open_context(ContextTarget::Group { group: floated });
        capture_popover(
            menu.upcast_ref(),
            &format!("{dir}/group-menu-{theme:?}.png"),
        );
        for label in ["Add built-in panel", "Add Toolbar"] {
            let trigger = find_menu_item(menu.upcast_ref(), label).unwrap();
            assert!(trigger.activate());
            pump(100);
            capture_popover(
                menu.upcast_ref(),
                &format!("{dir}/submenu-{label}-{theme:?}.png"),
            );
            menu.set_visible_submenu(Some("main"));
            pump(50);
        }
        activate(&menu, "Tools toolbar");
        assert_eq!(group(Panel::Toolbar), floated);
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        assert_ne!(group(Panel::Toolbar), floated);
        open_context(ContextTarget::Group { group: floated });
        activate(&menu, "Layers panel");
        assert_eq!(group(Panel::Layers), floated);
        // Addition grows the native allocation to fit measured tab labels.
        pump(150);
        let root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == floated)
            .unwrap()
            .root
            .clone();
        let labels: i32 = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == floated)
            .unwrap()
            .tabs
            .iter()
            .map(|(_, t)| t.measure(gtk::Orientation::Horizontal, -1).1)
            .sum();
        assert!(root.width() >= labels + 20);
        assert!(root.width() as f32 > narrow_width);
        snapshot(&format!("floating-tab-group-{theme:?}"));
        // Manual sizing takes over again and leaves genuine empty header space.
        let resize = find_named(
            w.surface.upcast_ref(),
            &format!("floating-resize-{floated}-BottomRight"),
        )
        .unwrap();
        let resize_drag = begin_workspace_drag(&w, &resize, 3.0, 3.0);
        resize_drag.update([80.0f64, 0.0f64]);
        resize_drag.end();
        pump(100);
        let header = find_css(root.upcast_ref(), "dock-tabs").unwrap();
        let original = placement(panel).bounds;
        let grab = gtk::graphene::Point::new(header.width() as f32 - 28.0, 12.0);
        let anchor = header.compute_point(&w.surface, &grab).unwrap();
        let area = w.resolved().work_area;
        // Release in free canvas, not within the neighboring sidebar's snap zone.
        let dx = area.x + area.width * 0.5 - anchor.x();
        let drag = begin_workspace_drag(&w, &header, grab.x(), grab.y());
        drag.update([dx as f64, 20.0f64]);
        pump(100);
        assert_eq!(live().x, original.x + dx);
        assert_eq!(
            w.groups
                .borrow()
                .iter()
                .find(|g| g.id == floated)
                .unwrap()
                .root,
            root
        );
        assert!(
            w.drop_hint.borrow().is_none(),
            "Free movement must end outside snap targets"
        );
        drag.end();
        pump(100);
        let resize = find_named(
            w.surface.upcast_ref(),
            &format!("floating-resize-{floated}-BottomRight"),
        )
        .unwrap();
        let before = placement(panel).bounds;
        let drag = begin_workspace_drag(&w, &resize, 3.0, 3.0);
        drag.update([30.0f64, 50.0f64]);
        pump(100);
        assert_eq!(placement(panel).bounds.width, before.width + 30.0);
        drag.end();
        let tall = placement(panel).bounds.height;
        let tab = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == floated)
            .unwrap()
            .tabs
            .iter()
            .find(|(p, _)| *p == panel)
            .unwrap()
            .1
            .clone();
        click(&tab);
        assert_eq!(placement(panel).bounds.height, tall);
        open_context(ContextTarget::Panel { panel });
        assert!(menu_action(&menu.menu_model().unwrap(), "Rename Painting toolbar…").is_none());
        activate(&menu, "Configure Painting toolbar…");
        snapshot(&format!("toolbar-configuration-{theme:?}"));
        send(CustomizationAction::CloseExpanded);
        send(CustomizationAction::SetTabStyle {
            group: floated,
            style: TabStyle::Icon,
        });
        let expected = state(&w).workspace.layout.panel(panel).unwrap().icon();
        assert_eq!(
            tab.child()
                .unwrap()
                .first_child()
                .and_downcast::<gtk::Image>()
                .unwrap()
                .widget_name()
                .as_str(),
            format!("layer-{expected}-symbolic").as_str()
        );
        send(CustomizationAction::SetTabStyle {
            group: floated,
            style: TabStyle::Name,
        });
        w.dispatch(UiAction::Invoke {
            command: CommandId::ZenMode,
        });
        pump(200);
        assert!(!root.has_css_class("zen-hidden"));
        assert!(
            w.groups
                .borrow()
                .iter()
                .filter(|g| !g.floating)
                .all(|g| g.root.has_css_class("zen-hidden"))
        );
        snapshot(&format!("floating-zen-{theme:?}"));
        w.dispatch(UiAction::Invoke {
            command: CommandId::ZenMode,
        });
        open_context(ContextTarget::Ribbon { panel });
        activate(&menu, "Hide Painting toolbar");
        assert!(state(&w).workspace.layout.panel_group(panel).is_none());
        workspace_menu.popup();
        activate(&workspace_menu, "Painting toolbar");
        workspace_menu.popup();
        activate(&workspace_menu, "Manage Toolbars…");
        let list = named::<gtk::ListBox>(w.window.upcast_ref(), "managed-toolbars");
        let index = ui_session(&w)
            .toolbar_manager()
            .unwrap()
            .toolbars
            .iter()
            .position(|p| p.panel == panel)
            .unwrap();
        list.select_row(list.row_at_index(index as i32).as_ref());
        click(
            &named::<gtk::Button>(w.window.upcast_ref(), "delete-managed-toolbar"),
        );
        assert!(prompt().body().contains("Undo Layout Change"));
        snapshot(&format!("delete-{theme:?}"));
        confirm_prompt();
        assert!(state(&w).workspace.layout.panel(panel).is_err());
        send(CustomizationAction::CloseToolbarManager);
        workspace_menu.popup();
        activate(&workspace_menu, "Undo Layout Change");
        assert_eq!(
            state(&w).workspace.layout.panel(panel).unwrap().canonical_title(),
            "Painting"
        );
    }
    menu.popdown();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "workspace customization: requires a Wayland/Vulkan display"]
fn native_panel_customization() {
    let app = native_test_app("dev.layer.CustomizationTest");
    // This test checks native controls and static snapshots. Test expansion
    // animation separately; occluded popups do not receive animation frames.
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = fixture_workspace(&app);
    w.window.present();
    new_photo::ready(&w);
    pump(600);
    let dir = artifact_dir("../../artifacts/ui/customization");
    let send = |action| w.dispatch(UiAction::Customize { action });
    // Presentation checks have no Wayland input serial for a popup grab. Keep
    // this control/snapshot test independent of external desktop focus changes;
    // production retains native autohide and actions still dismiss the menu.
    for popover in w.popovers.borrow().iter().filter_map(|p| p.upgrade()) {
        if popover.has_css_class("panel-context-menu") {
            popover.set_autohide(false);
        }
    }
    let hold_count = Cell::new(0);
    let hold = |widget: &gtk::Widget, target: ContextTarget, x: f64, y: f64| {
        hold_count.set(hold_count.get() + 1);
        assert!(
            widget.width() > 0 && widget.height() > 0,
            "unallocated context target {}: {}x{}",
            widget.widget_name(),
            widget.width(),
            widget.height()
        );
        assert!(
            widget.pick(x, y, gtk::PickFlags::DEFAULT).is_some(),
            "unpickable context target {}: {}x{}, mapped={}, visible={}, sensitive={}",
            widget.widget_name(),
            widget.width(),
            widget.height(),
            widget.is_mapped(),
            widget.is_visible(),
            widget.is_sensitive()
        );
        // Native device arbitration is covered by the compositor input suites.
        w.show_context(widget, target, x, y);
        pump(150);
    };
    let context = || {
        w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .find_map(|p| {
                (p.is_visible() && p.has_css_class("panel-context-menu"))
                    .then(|| p.downcast::<gtk::PopoverMenu>().ok())
                    .flatten()
            })
            .unwrap_or_else(|| {
                panic!(
                    "Context menu did not stay open after hold {}",
                    hold_count.get()
                )
            })
    };
    let snapshot_popover = |popover: &gtk::Popover, file: &str| {
        capture_popover(popover, &format!("{dir}/{file}.png"));
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let mut fixture = state(&w).workspace;
        fixture.layout.rename_toolbar(Panel::Toolbar, "Tools").unwrap();
        w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture) });
        send(CustomizationAction::SetControlVisible {
            panel: Panel::Sizes,
            control: PanelControl::BrushOpacity,
            visible: false,
        });
        pump(250);
        capture_reference(&w, &format!("{dir}/initial-{theme:?}.png"), 1.0);
        let initial = state(&w).workspace;
        let tab = w
            .groups
            .borrow()
            .iter()
            .flat_map(|g| &g.tabs)
            .find(|(p, _)| *p == Panel::Sizes)
            .unwrap()
            .1
            .clone();
        hold(
            tab.upcast_ref(),
            ContextTarget::Panel {
                panel: Panel::Sizes,
            },
            12.0,
            12.0,
        );
        let menu = context();
        snapshot_popover(menu.upcast_ref(), &format!("panel-menu-{theme:?}"));
        assert!(menu_action(&menu.menu_model().unwrap(), "Icons only").is_none());
        menu.popdown();
        let group = state(&w)
            .workspace
            .layout
            .panel_group(Panel::Sizes)
            .unwrap();
        let root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .root
            .clone();
        let header = find_css(root.upcast_ref(), "dock-tabs").unwrap();
        hold(
            &header,
            ContextTarget::Group { group },
            header.width() as f64 - 10.0,
            12.0,
        );
        let menu = context();
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "Icons only").unwrap(),
            None,
        )
        .unwrap();
        pump(100);
        assert_eq!(
            state(&w).workspace.layout.group_tab_style(group).unwrap(),
            TabStyle::Icon
        );
        assert_eq!(
            tab.child()
                .unwrap()
                .first_child()
                .and_downcast::<gtk::Image>()
                .unwrap()
                .widget_name()
                .as_str(),
            "layer-size-symbolic"
        );
        send(CustomizationAction::SetTabStyle {
            group,
            style: TabStyle::Name,
        });
        w.dispatch(UiAction::SelectPanelTab {
            group,
            panel: Panel::Sizes,
        });
        let before_expansion = state(&w).workspace.layout.bands;

        let original_panel = w.panel_widget(Panel::Sizes);
        let original_parent = original_panel.parent().unwrap();
        let original_root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.panels.contains(&Panel::Sizes))
            .unwrap()
            .root
            .clone();
        send(CustomizationAction::ShowAllControls {
            panel: Panel::Sizes,
        });
        pump(300);
        capture_reference(
            &w,
            &format!("{dir}/expanded-sizes-before-{theme:?}.png"),
            1.0,
        );
        let inspector = original_root;
        assert!(inspector.has_css_class("expanded-panel"));
        assert_eq!(original_panel.parent().unwrap(), original_parent);
        assert!(
            w.popovers
                .borrow()
                .iter()
                .filter_map(|p| p.upgrade())
                .all(|p| !p.has_css_class("expanded-panel"))
        );
        assert_eq!(state(&w).workspace.layout.bands, before_expansion);
        let compact_opacity = find_named(
            original_panel.upcast_ref(),
            "panel-field-Sizes-BrushOpacity",
        )
        .unwrap();
        assert!(!compact_opacity.is_visible());
        let opacity = find_named(inspector.upcast_ref(), "configure-Sizes-BrushOpacity").unwrap();
        assert!(opacity.is_visible());
        let input = opacity
            .last_child()
            .and_downcast::<crate::number_control::NumberControl>()
            .unwrap();
        input.set_value(0.42);
        input.emit_by_name::<()>("value-changed", &[]);
        assert!((state(&w).brush.opacity - 0.42).abs() < 0.001);
        let visible = named::<gtk::CheckButton>(inspector.upcast_ref(), "panel-visible-BrushOpacity");
        visible.set_active(true);
        pump(100);
        assert!(compact_opacity.is_visible());
        capture_reference(&w, &format!("{dir}/expanded-sizes-{theme:?}.png"), 1.0);
        assert!(w.reveal_chrome_at(850.0, 850.0));
        pump(300);
        assert!(state(&w).customization.expanded.is_none());
        assert!(
            w.panel_widget(Panel::Sizes)
                .parent()
                .is_some_and(|p| p.is::<gtk::Stack>())
        );
        assert!(compact_opacity.is_visible());
        assert_eq!(original_panel.parent().unwrap(), original_parent);

        let header = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .stack
            .parent()
            .unwrap()
            .first_child()
            .unwrap();
        hold(
            &header,
            ContextTarget::Group { group },
            (header.width() - 12) as f64,
            12.0,
        );
        let menu = context();
        snapshot_popover(menu.upcast_ref(), &format!("group-menu-{theme:?}"));
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "New Toolbar…").unwrap(),
            None,
        )
        .unwrap();
        pump(250);
        let name = named::<adw::EntryRow>(w.window.upcast_ref(), "toolbar-name");
        let confirm = named::<gtk::Button>(w.window.upcast_ref(), "confirm-tools");
        assert!(!confirm.is_sensitive());
        name.set_text("Tools");
        assert!(
            ui_session(&w)
                .tool_picker()
                .unwrap()
                .error
                .is_some()
        );
        name.set_text(&format!("Illustration {theme:?}"));
        let search = named::<gtk::SearchEntry>(w.window.upcast_ref(), "tool-search");
        search.set_text("pencil");
        pump(250);
        let choices = ui_session(&w)
            .tool_picker()
            .unwrap()
            .choices
            .into_iter()
            .filter(|choice| matches!(choice.control, ToolbarControl::Brush { .. }))
            .collect::<Vec<_>>();
        assert!(!choices.is_empty());
        for choice in choices.iter().take(2) {
            named::<gtk::CheckButton>(w.window.upcast_ref(), &format!("tool-choice-{}", serde_json::to_string(&choice.control).unwrap()))
            .set_active(true);
        }
        assert!(confirm.is_sensitive());
        capture_reference(&w, &format!("{dir}/tool-picker-{theme:?}.png"), 1.0);
        click(&confirm);
        pump(250); // Finish AdwDialog's closing animation before targeting the ribbon.
        let layout = state(&w).workspace.layout;
        let panel = layout
            .panels
            .iter()
            .find(|p| p.canonical_title() == format!("Illustration {theme:?}"))
            .unwrap()
            .id;
        let toolbar = w.panel_widget(panel).downcast::<TileStrip>().unwrap();
        assert_eq!(toolbar.overflow(), gtk::Overflow::Hidden);
        let tile = layout.panel(panel).unwrap().tiles()[0].id;
        let button = named::<gtk::Button>(toolbar.upcast_ref(), &format!("tile-{tile}"));
        click(&button);
        assert_eq!(
            state(&w).brush.preset,
            match choices[0].control {
                ToolbarControl::Brush { id } => id,
                _ => panic!("brush choice"),
            }
        );
        let root = button.parent().unwrap();
        hold(&root, ContextTarget::Tile { panel, tile }, 10.0, 10.0);
        let menu = context();
        snapshot_popover(menu.upcast_ref(), &format!("tile-menu-{theme:?}"));
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "Insert Tools…").unwrap(),
            None,
        )
        .unwrap();
        pump(200);
        send(CustomizationAction::PickerSearch { query: "".into() });
        send(CustomizationAction::PickerSelect {
            control: ToolbarControl::Size { tenths: 600 },
            selected: true,
        });
        send(CustomizationAction::ConfirmTools);
        assert_eq!(
            state(&w).workspace.layout.panel(panel).unwrap().tiles()[0].control,
            ToolbarControl::Size { tenths: 600 }
        );
        let moved = state(&w).workspace.layout.panel(panel).unwrap().tiles()[0].id;
        let group = w
            .resolved()
            .groups
            .iter()
            .find(|g| g.panels.contains(&Panel::Toolbar))
            .unwrap()
            .id;
        w.dispatch(UiAction::SelectPanelTab {
            group,
            panel: Panel::Toolbar,
        });
        send(CustomizationAction::CloseExpanded);
        pump(250);
        let resolved = w.resolved();
        let destination = resolved
            .groups
            .iter()
            .find(|g| g.active == Panel::Toolbar)
            .unwrap();
        let line = destination
            .tiles
            .as_ref()
            .unwrap()
            .insertion
            .last()
            .unwrap();
        let point = [
            destination.bounds.x + line.x + line.width * 0.5,
            destination.bounds.y
                + if destination.tabs_visible {
                    TAB_BAR_HEIGHT
                } else {
                    0.0
                }
                + line.y
                + line.height * 0.5,
        ];
        let item = DockItem::Tile { panel, tile: moved };
        let hint = w.drop_at(point[0], point[1], item).unwrap();
        *w.drop_hint.borrow_mut() = Some(hint.clone());
        capture_reference(&w, &format!("{dir}/tile-drop-{theme:?}.png"), 1.0);
        // Captured workspace gestures commit the shared validated move.
        w.dispatch(item.move_action(
            hint.target,
            [w.surface.width() as f32, w.surface.height() as f32],
        ));
        w.clear_drop();
        assert_eq!(
            state(&w)
                .workspace
                .layout
                .panel(Panel::Toolbar)
                .unwrap()
                .tiles()
                .last()
                .unwrap()
                .id,
            moved
        );
        w.dispatch(UiAction::MovePanel {
            viewport: [1200.0, 900.0],
            panel,
            target: DockTarget::Edge {
                edge: Edge::Left,
                outer: false,
            },
        });
        pump(200);
        capture_reference(&w, &format!("{dir}/custom-workspace-{theme:?}.png"), 1.0);
        let saved = state(&w).workspace;
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(initial),
        });
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(saved.clone()),
        });
        assert_eq!(state(&w).workspace, saved);
        w.dispatch(UiAction::Invoke {
            command: CommandId::ResetLayout,
        });
        assert!(state(&w).workspace.layout.panel(panel).is_ok());
        for tile in state(&w)
            .workspace
            .layout
            .panel(panel)
            .unwrap()
            .tiles()
            .to_vec()
        {
            send(CustomizationAction::RemoveTool {
                panel,
                tile: tile.id,
            });
        }
        let group = w
            .resolved()
            .groups
            .iter()
            .find(|g| g.panels.contains(&panel))
            .unwrap()
            .id;
        w.dispatch(UiAction::SelectPanelTab { group, panel });
        pump(150);
        capture_reference(&w, &format!("{dir}/empty-toolbar-{theme:?}.png"), 1.0);
        hold(
            &w.panel_widget(panel),
            ContextTarget::Ribbon { panel },
            12.0,
            12.0,
        );
        let menu = context();
        snapshot_popover(menu.upcast_ref(), &format!("ribbon-menu-{theme:?}"));
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "Add Tools…").unwrap(),
            None,
        )
        .unwrap();
        pump(150);
        assert!(
            ui_session(&w)
                .tool_picker()
                .is_some()
        );
        send(CustomizationAction::CancelTools);
        pump(250);
        // A large tabbed ribbon keeps all tiles but clips overflow rather than
        // installing a scroller that would compete with drag-to-reorder.
        let mut overflow = state(&w).workspace;
        let group = overflow.layout.panel_group(Panel::Sizes).unwrap();
        let many = overflow
            .layout
            .add_toolbar(
                Some(group),
                &format!("Many tools {theme:?}"),
                &vec![
                    ToolbarControl::Command {
                        command: CommandId::Brush
                    };
                    80
                ],
            )
            .unwrap();
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(overflow),
        });
        pump(200);
        let strip = w.panel_widget(many);
        assert_eq!(strip.overflow(), gtk::Overflow::Hidden);
        assert!(strip.is::<TileStrip>());
        let resolved = w.resolved();
        let group = resolved.groups.iter().find(|g| g.active == many).unwrap();
        let tiles = &group.tiles.as_ref().unwrap().tiles;
        assert_eq!(tiles.len(), 80);
        assert_eq!(tiles.last().unwrap().height, 0.);
        capture_reference(&w, &format!("{dir}/clipped-ribbon-{theme:?}.png"), 1.0);
        w.dispatch(UiAction::MovePanel {
            panel: many,
            target: DockTarget::Edge {
                edge: Edge::Left,
                outer: false,
            },
            viewport: [1200.0, 900.0],
        });
        pump(200);
        assert!(w.panel_widget(many).width() > (TILE_SIZE * 2.0) as i32);
        capture_reference(&w, &format!("{dir}/wrapped-ribbon-{theme:?}.png"), 1.0);
    }
    w.window.destroy();
    pump(150);
}

#[test]
#[ignore = "toolbar manager: requires a private Wayland/Vulkan display"]
fn native_toolbar_manager() {
    let app = native_test_app("art.capycanvas.ToolbarManagerTest");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    new_photo::ready(&w);
    until(|| w.window.is_maximized(), "native toolbar window allocated");
    let mut input = RemoteInput::new();
    input.ready();
    input.click(screen_point(w.header.root.upcast_ref(), &w.window, [0.5, 0.5]));
    until(|| w.window.is_active(), "native toolbar window activated");
    pump(500);
    let initial = state(&w).workspace;
    let send = |action| w.dispatch(UiAction::Customize { action });
    let model = || {
        ui_session(&w)
            .toolbar_manager()
            .unwrap()
    };
    let dir = artifact_dir("../../artifacts/ui/toolbar-manager");
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(initial.clone()),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for name in ["Sketching", "Painting"] {
            send(CustomizationAction::DuplicateToolbar {
                panel: Panel::Toolbar,
            });
            pump(100);
            send(CustomizationAction::ToolbarName { name: name.into() });
            let prompt = named::<adw::AlertDialog>(w.window.upcast_ref(), "toolbar-dialog");
            let label = ui_session(&w).toolbar_prompt().unwrap().confirm_label;
            input.click(screen_point(find_button(prompt.upcast_ref(), &label).unwrap().upcast_ref(), &w.window, [0.5, 0.5]));
            until(|| ui_session(&w).toolbar_prompt().is_none(), "native duplicate toolbar confirmation");
            pump(200);
        }
        let hidden = state(&w).workspace.layout.panels.last().unwrap().id;
        send(CustomizationAction::SetPanelVisible {
            panel: hidden,
            visible: false,
        });
        let before = serde_json::to_value(state(&w).workspace).unwrap();
        let menu = named::<gtk::PopoverMenu>(w.window.upcast_ref(), "workspace-menu");
        menu.set_autohide(false);
        menu.popup();
        pump(150);
        capture_popover(
            menu.upcast_ref(),
            &format!("{dir}/workspace-menu-{theme:?}.png"),
        );
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "Manage Toolbars…").unwrap(),
            None,
        )
        .unwrap();
        pump(300);

        let dialog = named::<adw::Dialog>(w.window.upcast_ref(), "toolbar-manager");
        let list = named::<gtk::ListBox>(dialog.upcast_ref(), "managed-toolbars");
        let delete = named::<gtk::Button>(dialog.upcast_ref(), "delete-managed-toolbar");
        assert_eq!(model().toolbars.len(), 3);
        assert!(!delete.is_sensitive());
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-initial.png"), 1.0);
        list.select_row(list.row_at_index(2).as_ref());
        assert_eq!(model().selected, Some(hidden));
        assert!(delete.is_sensitive());
        pump(100);
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-selected.png"), 1.0);
        click(&delete);
        pump(200);
        let prompt = named::<adw::AlertDialog>(w.window.upcast_ref(), "toolbar-dialog");
        assert_eq!(w.window.visible_dialog(), Some(prompt.clone().upcast()));
        assert!(prompt.body().contains("Painting"));
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-confirm.png"), 1.0);
        click(&find_button(prompt.upcast_ref(), "Cancel").unwrap());
        pump(200);
        assert_eq!(serde_json::to_value(state(&w).workspace).unwrap(), before);
        assert_eq!(model().selected, Some(hidden));
        click(&delete);
        pump(150);
        click(&find_button(prompt.upcast_ref(), "Delete Toolbar").unwrap());
        pump(200);
        assert_eq!(model().toolbars.len(), 2);
        assert!(model().selected.is_none());
        assert!(!delete.is_sensitive());
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-deleted.png"), 1.0);
        while !model().toolbars.is_empty() {
            list.select_row(list.row_at_index(0).as_ref());
            click(&delete);
            pump(150);
            click(&find_button(prompt.upcast_ref(), "Delete Toolbar").unwrap());
            pump(150);
        }
        assert!(!delete.is_sensitive());
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-empty.png"), 1.0);
        dialog.close();
        pump(300);
        assert!(
            ui_session(&w)
                .toolbar_manager()
                .is_none()
        );
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        assert!(
            state(&w)
                .workspace
                .layout
                .panels
                .iter()
                .any(|p| p.id.kind() == PanelKind::Tiles)
        );
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "native menu sections: requires a Wayland/Vulkan display"]
fn native_menu_sections() {
    let app = native_test_app("dev.layer.MenuTest");
    let w = fixture_workspace(&app);
    w.window.present();
    new_photo::ready(&w);
    // Exercise both the menu bar and the configurable Main Menu component.
    let mut workspace = state(&w).workspace;
    let first = workspace.layout.header.zones[HeaderZone::Left.index()].first().unwrap().id;
    workspace.layout.header.add(HeaderZone::Left, Some(first), &[HeaderItem::Menu]).unwrap();
    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
    pump(300);
    let dir = artifact_dir("../../artifacts/ui/menus");
    let open = |id: ApplicationMenu| {
        let popup = w
            .popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .filter_map(|p| p.downcast::<gtk::PopoverMenu>().ok())
            .find(|p| p.parent().unwrap().tooltip_text().as_deref() == Some(&id.canonical_label()))
            .unwrap();
        popup.popup();
        pump(100);
        popup
    };
    let activate = |popup: &gtk::PopoverMenu, label: &str| {
        let action = menu_action(&popup.menu_model().unwrap(), label).unwrap();
        popup.activate_action(&action, None).unwrap();
        pump(100);
    };
    fn check(model: &gtk::gio::MenuModel, sections: &[Vec<ContextMenuItem>], drawings: bool) {
        let sections: Vec<_> = sections.iter().filter(|s| !s.is_empty()).collect();
        assert_eq!(model.n_items() as usize, sections.len() + usize::from(drawings));
        if drawings {
            let section = model.item_link(sections.len() as i32, "section").unwrap();
            assert_eq!(section.n_items(), 1);
            assert_eq!(section.item_attribute_value(0, "action", None).unwrap().str(), Some("context.drawings"));
        }
        for (s, items) in sections.iter().enumerate() {
            let section = model.item_link(s as i32, "section").unwrap();
            assert_eq!(section.n_items() as usize, items.len());
            for (i, item) in items.iter().enumerate() {
                assert_eq!(
                    section
                        .item_attribute_value(i as i32, "label", None)
                        .unwrap()
                        .str(),
                    Some(item.label.as_str())
                );
                if item.action.is_none() && item.enabled {
                    check(
                        &section.item_link(i as i32, "submenu").unwrap(),
                        &item.sections,
                        false,
                    );
                } else {
                    assert!(section.item_link(i as i32, "submenu").is_none());
                    assert!(
                        section
                            .item_attribute_value(i as i32, "custom", None)
                            .is_none()
                    );
                    assert_eq!(
                        section
                            .item_attribute_value(i as i32, "accel", None)
                            .and_then(|v| v.get::<String>()),
                        item.bindings.first().map(native_accelerator)
                    );
                    assert!(
                        section
                            .item_attribute_value(i as i32, "action", None)
                            .is_some()
                    );
                }
            }
        }
    }
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for id in ApplicationMenu::ALL
            .into_iter()
            .chain([ApplicationMenu::Primary])
        {
            let menu = open(id);
            let expected = ui_session(&w)
                .application_menu(id);
            check(&menu.menu_model().unwrap(), &expected.sections, matches!(id, ApplicationMenu::Primary | ApplicationMenu::Window));
            if id == ApplicationMenu::Select {
                for label in ["Load Selection", "Replace Selection Layer from Current Selection"] {
                    assert!(!find_menu_item(menu.upcast_ref(), label).unwrap().is_sensitive());
                }
                assert!(find_menu_item(menu.upcast_ref(), "Modify").is_none());
                assert!(find_menu_item(menu.upcast_ref(), &CommandId::GrowSelection.label()).is_some());
                assert!(find_menu_item(menu.upcast_ref(), &CommandId::ShrinkSelection.label()).is_some());
            }
            if id == ApplicationMenu::View {
                assert!(
                    menu_action(&menu.menu_model().unwrap(), &CommandId::ToggleTheme.label())
                        .is_none()
                );
            }
            pump(200);
            capture_popover(
                menu.upcast_ref(),
                &format!("{dir}/{}-{theme:?}.png", id.canonical_label()),
            );
            menu.popdown();
            pump(100);
        }
    }
    activate(&open(ApplicationMenu::Select), &CommandId::SelectAll.label());
    assert!(state(&w).layer_tools.has_selection);
    activate(
        &open(ApplicationMenu::Edit),
        &CommandId::FillSelection.label(),
    );
    let checkpoint = ui_session(&w)
        .engine()
        .checkpoint();
    activate(&open(ApplicationMenu::Edit), &CommandId::ClearLayer.label());
    assert_ne!(
        ui_session(&w)
            .engine()
            .checkpoint(),
        checkpoint
    );
    activate(&open(ApplicationMenu::Edit), &CommandId::Undo.label());
    assert_eq!(
        ui_session(&w)
            .engine()
            .checkpoint(),
        checkpoint
    );
    activate(&open(ApplicationMenu::Select), &CommandId::Deselect.label());
    assert!(!state(&w).layer_tools.has_selection);
    let filter = state(&w).adjustments[0].label.clone();
    activate(&open(ApplicationMenu::Filter), &filter);
    {
        let gpu = w.gpu.borrow();
        let doc = gpu.as_ref().unwrap().session.engine().document();
        assert_eq!(
            doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap().content,
            layer_core::authored::OccurrenceContent::Effect(doc.artwork.effects.iter().next_back().unwrap().0)
        );
    }
    // Reopening must refresh removed shortcuts in every menu using the command.
    let mut settings = state(&w).settings;
    settings
        .shortcuts
        .insert(CommandId::Settings.shortcut_id(), vec![]);
    w.dispatch(UiAction::RestoreSettings { settings });
    for id in [ApplicationMenu::Primary, ApplicationMenu::Edit] {
        let menu = open(id);
        let expected = ui_session(&w)
            .application_menu(id);
        check(&menu.menu_model().unwrap(), &expected.sections, matches!(id, ApplicationMenu::Primary | ApplicationMenu::Window));
        menu.popdown();
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "two-column panel expansion: requires a Wayland/Vulkan display"]
fn native_panel_expansion() {
    let app = native_test_app("dev.layer.ExpansionTest");
    // Static geometry/picking captures also verify the reduced-motion path.
    // Interpolation fractions are covered by the shared layout tests.
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = fixture_workspace(&app);
    w.window.present();
    pump(600);
    let initial = state(&w).workspace;
    let dir = artifact_dir("../../artifacts/ui/customization");
    let tap_tab = |panel| {
        let tab = w
            .groups
            .borrow()
            .iter()
            .flat_map(|g| &g.tabs)
            .find(|(p, _)| *p == panel)
            .unwrap()
            .1
            .clone();
        let bounds = tab.compute_bounds(&w.surface).unwrap();
        let reply = w.chrome_event(ChromeEvent::Contact {
            position: [
                bounds.x() + bounds.width() * 0.5,
                bounds.y() + bounds.height() * 0.5,
            ],
            canvas: false,
        });
        assert!(
            !reply.handled,
            "tab press must remain available for native drag/hold"
        );
        click(&tab);
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(initial.clone()),
            });
            w.dispatch(UiAction::MovePanel {
                panel: Panel::Sizes,
                target: DockTarget::Edge { edge, outer: false },
                viewport: [1200.0, 900.0],
            });
            pump(100);
            let saved = state(&w).workspace;
            let panel = w.panel_widget(Panel::Sizes);
            let parent = panel.parent().unwrap();
            let root = w
                .groups
                .borrow()
                .iter()
                .find(|g| g.panels.contains(&Panel::Sizes))
                .unwrap()
                .root
                .clone();
            tap_tab(Panel::Sizes);
            pump(300);
            capture_reference(&w, &format!("{dir}/expanded-{edge:?}-{theme:?}.png"), 1.0);
            let placement = w.customization.placement().unwrap();
            assert_eq!(panel.parent().unwrap(), parent);
            assert_eq!(
                placement.preview.height,
                placement.configuration.height + TAB_BAR_HEIGHT
            );
            assert_eq!(placement.configuration.y, TAB_BAR_HEIGHT);
            assert!(placement.configuration.width > placement.preview.width);
            let point = gtk::graphene::Point::new(
                placement.bounds.x + placement.configuration.x + 16.0,
                placement.bounds.y + 80.0,
            );
            let picked = w
                .surface
                .pick(point.x() as f64, point.y() as f64, gtk::PickFlags::DEFAULT)
                .unwrap();
            assert!(
                picked.is_ancestor(&root),
                "configuration must be the actual raised group, not a popover"
            );
            let check = named::<gtk::CheckButton>(root.upcast_ref(), "panel-visible-BrushColor");
            check.set_active(true);
            pump(100);
            assert!(
                find_named(&panel, "panel-field-Sizes-BrushColor")
                    .unwrap()
                    .is_visible()
            );
            check.set_active(false);
            tap_tab(Panel::Sizes);
            pump(300);
            assert!(w.customization.placement().is_none());
            assert_eq!(state(&w).workspace, saved);
            assert_eq!(panel.parent().unwrap(), parent);
        }
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(initial.clone()),
        });
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Sizes,
            target: DockTarget::Tab {
                group: 8,
                index: None,
            },
            viewport: [1200.0, 900.0],
        });
        pump(100);
        let preview_parent = w.panel_widget(Panel::Sizes).parent().unwrap();
        for (panel, name, concave) in [
            (Panel::Sizes, "second", false),
            (Panel::Layers, "first", true),
        ] {
            tap_tab(panel);
            pump(150);
            capture_reference(
                &w,
                &format!("{dir}/expanded-left-{name}-tab-{theme:?}.png"),
                1.0,
            );
            let placement = w.customization.placement().unwrap();
            assert_eq!(placement.concave_join, concave);
            assert_eq!(state(&w).customization.expanded, Some(panel));
            assert_eq!(
                w.panel_widget(Panel::Sizes).parent().unwrap(),
                preview_parent
            );
        }
        tap_tab(Panel::Layers);
        assert!(state(&w).customization.expanded.is_none());
    }
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    w.dragging.set(true);
    w.update_zen();
    assert!(!w.interact(UiInput::Blur).chrome_hidden);
    for group in w.groups.borrow().iter() {
        assert!(!group.root.has_css_class("zen-hidden"));
        assert!(group.root.can_target());
    }
    capture_reference(&w, &format!("{dir}/zen-drag-Light.png"), 1.0);
    w.dragging.set(false);
    w.update_zen();
    assert!(
        w.groups
            .borrow()
            .iter()
            .all(|g| g.root.has_css_class("zen-hidden"))
    );
    let hide = w.chrome_event(ChromeEvent::Contact {
        position: [600.0, 350.0],
        canvas: true,
    });
    assert!(!hide.handled && hide.chrome_hidden);
    assert!(
        w.groups
            .borrow()
            .iter()
            .all(|g| g.root.has_css_class("zen-hidden"))
    );
}

#[test]
#[ignore = "GTK visual reference for the web: requires a Wayland display"]
fn native_web_parity_reference() {
    let app = native_test_app("dev.layer.ParityTest");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    std::fs::create_dir_all("../../artifacts/ui/parity").unwrap();
    fn record(widget: &gtk::Widget, root: &gtk::Widget) -> serde_json::Value {
        let b = widget.compute_bounds(root).unwrap();
        let mut children = Vec::new();
        let mut child = widget.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if c.is_visible() && c.is_child_visible() {
                children.push(record(&c, root));
            }
        }
        serde_json::json!({
            "type": widget.type_().name(),
            "name": widget.widget_name().to_string(),
            "css": widget.css_classes().iter().map(ToString::to_string).collect::<Vec<_>>(),
            "bounds": [b.x(), b.y(), b.width(), b.height()],
            "font": widget.pango_context().font_description().map(|f| f.to_string()),
            "text": widget.downcast_ref::<gtk::Label>().map(|l| l.text().to_string()),
            "icon": widget.downcast_ref::<gtk::Image>().and_then(crate::icons::name).map(|s| s.to_string()),
            "children": children,
        })
    }
    for (scheme, name) in [
        (adw::ColorScheme::ForceDark, "dark"),
        (adw::ColorScheme::ForceLight, "light"),
    ] {
        for modal in [false, true] {
            adw::StyleManager::default().set_color_scheme(scheme);
            adw::StyleManager::for_display(&gdk::Display::default().unwrap())
                .set_color_scheme(adw::ColorScheme::Default);
            let w = fixture_workspace(&app);
            w.window.present();
            assert!(w.gpu.borrow().is_some());
            if modal {
                w.dispatch(UiAction::Invoke {
                    command: CommandId::Settings,
                });
            }
            pump(2500);
            let name = if modal {
                format!("settings-{name}")
            } else {
                name.to_string()
            };
            capture_reference(
                &w,
                &format!("../../artifacts/ui/parity/gtk-{name}.png"),
                1.0,
            );
            if !modal {
                capture_reference(
                    &w,
                    &format!("../../artifacts/ui/parity/gtk-{name}-2x.png"),
                    2.0,
                );
            }
            let mut reference = record(w.window.upcast_ref(), w.window.upcast_ref());
            reference["scale"] = serde_json::json!(w.area.scale_factor());
            reference["camera"] = serde_json::to_value(state(&w).camera).unwrap();
            std::fs::write(
                format!("../../artifacts/ui/parity/gtk-{name}.json"),
                serde_json::to_vec(&reference).unwrap(),
            )
            .unwrap();
            w.window.destroy();
            pump(100);
        }
    }
}

#[test]
#[ignore = "tab visibility and bottom grips: requires a private Wayland/Vulkan display"]
fn native_hidden_tabs() {
    let app = native_test_app("art.capycanvas.HiddenTabsTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(700);
    let initial = state(&w).workspace;
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let dir = artifact_dir("../../artifacts/ui/workspace-management/gtk");
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(initial.clone()),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let panel = Panel::Sizes;
        let group = state(&w).workspace.layout.panel_group(panel).unwrap();
        for action in [
            CustomizationAction::SetTabStyle {
                group,
                style: TabStyle::Icon,
            },
            CustomizationAction::SetTabHidden {
                panel,
                hidden: true,
            },
        ] {
            w.dispatch(UiAction::Customize { action });
        }
        pump(150);
        let footer = || {
            find_named(
                w.surface.upcast_ref(),
                &format!("panel-footer-grip-{group}"),
            )
            .unwrap()
        };
        let handle = footer();
        let b = handle.compute_bounds(&w.surface).unwrap();
        assert_eq!(b.height(), 20.0);
        assert!(
            matches!(w.drag_target_at([b.x() + b.width() / 2.0, b.y() + 10.0]), Some(DragTarget::Dock(DockItem::Group { group: id })) if id == group)
        );
        assert!(
            w.groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .tabs
                .is_empty()
        );
        capture_reference(&w, &format!("{dir}/tab-hidden-docked-{theme:?}.png"), 1.0);
        // The footer menu exposes name/icon selection and Show tab bar.
        // Compositor tests cover the touch/pen-only hold binding.
        w.show_context(
            &handle,
            ContextTarget::Group { group },
            b.width() as f64 * 0.5,
            10.0,
        );
        pump(150);
        let menu = w
            .popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .find(|p| p.has_css_class("panel-context-menu"))
            .unwrap();
        capture_popover(&menu, &format!("{dir}/tab-hidden-menu-{theme:?}.png"));
        let popup = menu.clone().downcast::<gtk::PopoverMenu>().unwrap();
        let action = menu_action(&popup.menu_model().unwrap(), "Configure Brush size panel…")
            .expect("Hidden panels expose their configuration action");
        popup.activate_action(&action, None).unwrap();
        pump(300);
        assert_eq!(state(&w).customization.expanded, Some(panel));
        capture_reference(
            &w,
            &format!("{dir}/tab-hidden-configure-{theme:?}.png"),
            1.0,
        );
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::CloseExpanded,
        });
        pump(300);
        menu.popdown();
        pump(100);
        let drag = begin_workspace_drag(&w, &handle, b.width() * 0.5, 10.0);
        let target = [viewport[0] * 0.55, viewport[1] * 0.55];
        drag.update([
            (target[0] - drag.origin[0]) as f64,
            (target[1] - drag.origin[1]) as f64,
        ]);
        pump(150);
        drag.end();
        pump(100);
        let placement = w
            .resolved()
            .groups
            .into_iter()
            .find(|g| g.id == group)
            .unwrap();
        assert!(placement.floating && !placement.tabs_visible);
        assert!(footer().is_mapped());
        capture_reference(&w, &format!("{dir}/tab-hidden-floating-{theme:?}.png"), 1.0);
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetTabHidden {
                panel,
                hidden: false,
            },
        });
        pump(150);
        let tab = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .tabs[0]
            .1
            .clone();
        let content = tab.child().unwrap();
        assert!(content.first_child().unwrap().is_visible());
        assert!(!content.last_child().unwrap().is_visible());
        assert!(
            find_named(
                w.surface.upcast_ref(),
                &format!("panel-footer-grip-{group}")
            )
            .is_none()
        );
        capture_reference(
            &w,
            &format!("{dir}/tab-restored-floating-{theme:?}.png"),
            1.0,
        );
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetTabHidden {
                panel,
                hidden: true,
            },
        });
        pump(100);
        let before_dock = state(&w).workspace;
        let grip = footer().compute_bounds(&w.surface).unwrap();
        for (phase, position) in [
            (
                layer_ui::ContactPhase::Down,
                [grip.x() + grip.width() * 0.5, grip.y() + 10.],
            ),
            (
                layer_ui::ContactPhase::Move,
                [viewport[0] - 1., viewport[1] * 0.5],
            ),
            (
                layer_ui::ContactPhase::Up,
                [viewport[0] - 1., viewport[1] * 0.5],
            ),
        ] {
            w.dispatch(UiAction::DragWorkspace {
                item: DockItem::Group { group },
                phase,
                position,
                viewport,
                tabs: vec![],
            });
        }
        pump(150);
        assert!(state(&w).workspace.layout.panel(panel).unwrap().hide_tab);
        assert_eq!(
            w.groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .tabs
                .len(),
            0
        );
        assert!(
            find_named(
                w.surface.upcast_ref(),
                &format!("panel-footer-grip-{group}")
            )
            .is_some()
        );
        capture_reference(
            &w,
            &format!("{dir}/tab-hidden-after-docking-{theme:?}.png"),
            1.0,
        );
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(before_dock).unwrap()
        );
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "native divider hit testing: requires a Wayland display"]
fn native_stacked_divider() {
    let app = native_test_app("art.capycanvas.DividerTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(800);
    let divider = w
        .resolved()
        .dividers
        .into_iter()
        .find(|d| d.id == 4)
        .unwrap();
    let b = divider.bounds;
    let point = [b.x + b.width * 0.5, b.y + b.height * 0.5];
    let picked = w
        .surface
        .pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT)
        .unwrap();
    let handle = w
        .surface
        .imp()
        .children
        .borrow()
        .iter()
        .find(|(slot, _)| *slot == Slot::Divider(4))
        .unwrap()
        .1
        .clone();
    assert_eq!(
        picked, handle,
        "the horizontal divider must own its complete hit area"
    );
    let drag = begin_workspace_drag(&w, &handle, b.width * 0.5, b.height * 0.5);
    for dy in [-100.0, 50.0, -70.0] {
        drag.update([0.0f64, (dy as f64)]);
        pump(50);
        let actual = handle.compute_bounds(&w.surface).unwrap();
        assert!(
            (actual.y() - b.y - dy).abs() <= 1.0,
            "divider y={} expected {}",
            actual.y(),
            b.y + dy
        );
    }
    drag.update([0.0f64, 0.0f64]);
    drag.end();
    let mut workspace = state(&w).workspace;
    if let DockNode::Split { id, .. } = &mut workspace.layout.bands[0].root {
        *id = 40;
    }
    let mut json = serde_json::to_value(&workspace).unwrap();
    json["layout"]["next_id"] = 41.into();
    workspace = serde_json::from_value(json).unwrap();
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    pump(100);
    assert!(
        w.surface
            .imp()
            .children
            .borrow()
            .iter()
            .any(|(s, _)| *s == Slot::Divider(40)),
        "same panels with a new split must replace the old native handle"
    );
    let tool = command(&w, CommandId::Brush);
    std::fs::create_dir_all("../../artifacts/ui/preferences").unwrap();
    capture_reference(&w, "../../artifacts/ui/preferences/gtk-typography.png", 1.0);
    for widget in [
        w.groups.borrow()[0].tabs[0]
            .1
            .clone()
            .upcast::<gtk::Widget>(),
        w.size_number.clone().upcast(),
        w.view_info.root.clone().upcast(),
    ] {
        let font = widget.pango_context().font_description().unwrap();
        assert!(
            (font.size() as f64 / gtk::pango::SCALE as f64 - UI_TEXT_PT as f64 * 4.0 / 3.0).abs()
                < 0.02,
            "expected {UI_TEXT_PT}pt, got {font}"
        );
    }
    assert_eq!([tool.width(), tool.height()], [36, 36]);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "multiple-window native teardown: requires a Wayland display"]
fn native_window_lifecycle() {
    let app = native_test_app("art.capycanvas.LifecycleTest");
    let windows: Rc<RefCell<Vec<Rc<Workspace>>>> = Rc::default();
    crate::install_actions(&app, &windows);
    app.activate_action("new-window", None);
    until(|| windows.borrow().first().is_some_and(|workspace| workspace.window.is_mapped()), "first lifecycle window mapped");
    let first = windows.borrow()[0].clone();
    pump(200);
    for _ in 0..12 {
        app.activate_action("new-window", None);
        until(|| windows.borrow().len() == 2 && windows.borrow()[1].window.is_mapped(), "next lifecycle window mapped");
        pump(100);
        let next = windows.borrow().last().unwrap().clone();
        next.window.destroy();
        pump(150);
        // Unrealize stops/joins the renderer while retaining the shared session
        // for possible remapping. Verify GPU ownership, not model destruction.
        assert!(next.gpu.borrow().as_ref().is_none_or(|g| g.session.engine().backend().worker_is_joined()));
        assert!(next.frame_timer.borrow().is_none());
        assert_eq!(windows.borrow().len(), 1);
    }
    first.window.destroy();
    pump(100);
    assert!(first.gpu.borrow().as_ref().is_none_or(|g| g.session.engine().backend().worker_is_joined()));
    assert!(first.frame_timer.borrow().is_none());
    assert!(windows.borrow().is_empty());
}

#[test]
#[ignore = "native GTK widgets: requires a Wayland display"]
fn native_ribbon_allocation() {
    adw::init().unwrap();
    let css = crate::stylesheet_provider();
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let strip = TileStrip::new();
    strip.add_css_class("toolbar-controls");
    let controls: Vec<_> = (1..=6)
        .map(|id| layer_ui::ToolbarTile {
            id,
            control: ToolbarControl::Command {
                command: CommandId::Undo,
            },
        })
        .collect();
    for _ in &controls {
        strip.append(&gtk::Button::from_icon_name("document-edit-symbolic"));
    }
    strip.set_tiles(&controls);
    strip.set_grip(&tiles::grip(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English).text(layer_ui::MessageId::DOCUMENTS_DELIVERY_DRAG_PANEL)));
    for (axis, edge) in [(Axis::Horizontal, Edge::Top), (Axis::Vertical, Edge::Left)] {
        let mut layout = DockLayout::default();
        layout.bands.retain(|b| b.id == 1);
        layout.bands[0].edge = edge;
        strip.configure(axis, true);
        for length in [96.0, 500.0] {
            let viewport = if axis == Axis::Horizontal {
                [length, 800.0]
            } else {
                [800.0, length]
            };
            let g = layout.resolve(viewport[0], viewport[1]).groups.remove(0);
            strip.allocate(g.bounds.width as i32, g.bounds.height as i32, -1, None);
            let expected = layer_ui::toolbar_tile_layout(
                g.bounds.width,
                g.bounds.height,
                axis,
                &controls,
                true,
                TileStyle::Small,
            );
            let mut child = strip.first_child();
            for b in expected.tiles.into_iter().chain(expected.grip) {
                let widget = child.unwrap();
                let actual = widget.compute_bounds(&strip).unwrap();
                assert_eq!(
                    (actual.x(), actual.y(), actual.width(), actual.height()),
                    (b.x, b.y, b.width, b.height)
                );
                child = widget.next_sibling();
            }
            assert!(child.is_none());
            assert_eq!(layout.bands[0].extent, TILE_SIZE + 6.0);
        }
    }
}

#[test]
#[ignore = "native sidebar, shortcut recording and persistence: requires a Wayland display"]
fn native_preferences_and_shortcuts() {
    if let Some(path) = crate::storage::roots().map(layer_host::StorageRoots::settings) {
        assert!(
            !path.exists(),
            "Use a fresh isolated preferences path for this test"
        );
    }
    let app = native_test_app("dev.layer.PreferencesTest");
    let windows: Rc<RefCell<Vec<Rc<Workspace>>>> = Rc::default();
    crate::install_actions(&app, &windows);
    app.activate_action("new-window", None);
    until(|| windows.borrow().first().is_some_and(|workspace| workspace.window.is_mapped()), "preferences window mapped");
    let w = windows.borrow()[0].clone();
    pump(300);
    assert_eq!(
        w.window.title().as_deref(),
        Some(format!("Untitled — {APP_NAME}").as_str())
    );
    click(&command(&w, CommandId::NewWindow));
    until(|| windows.borrow().len() == 2 && windows.borrow()[1].window.is_mapped(), "second preferences window mapped");
    assert_eq!(windows.borrow().len(), 2);
    let second = windows.borrow()[1].clone();
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    pump(100);
    assert_eq!(state(&second).settings, state(&w).settings);
    second.window.destroy();
    w.window.present();
    pump(100);
    assert_eq!(windows.borrow().len(), 1);
    let dir = artifact_dir("../../artifacts/ui/preferences");
    assert_eq!(w.preferences.dialog.content_width(), 1000);
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Appearance,
    });
    pump(300);
    find_named(
        w.preferences.dialog.upcast_ref(),
        "preferences-search-toggle",
    )
    .unwrap()
    .grab_focus();
    let controllers = w.window.observe_controllers();
    let keys = (0..controllers.n_items())
        .find_map(|i| {
            controllers
                .item(i)
                .and_downcast::<gtk::EventControllerKey>()
                .filter(|keys| keys.name().as_deref() == Some("workspace-shortcuts"))
        })
        .unwrap();
    assert!(keys.emit_by_name::<bool>(
        "key-pressed",
        &[&gdk::Key::P, &0u32, &gdk::ModifierType::SHIFT_MASK]
    ));
    keys.emit_by_name::<()>(
        "key-released",
        &[&gdk::Key::P, &0u32, &gdk::ModifierType::SHIFT_MASK],
    );
    pump(250);
    let search: gtk::SearchEntry = find_named(w.preferences.dialog.upcast_ref(), "settings-search")
        .unwrap()
        .downcast()
        .unwrap();
    assert_eq!(search.text().as_str(), "P");
    assert!(
        gtk::prelude::GtkWindowExt::focus(&w.window)
            .is_some_and(|focus| focus.is_ancestor(&search))
    );
    assert_eq!(search.position(), 1);
    assert!(!keys.emit_by_name::<bool>(
        "key-pressed",
        &[&gdk::Key::r, &0u32, &gdk::ModifierType::empty()]
    ));
    keys.emit_by_name::<()>(
        "key-released",
        &[&gdk::Key::r, &0u32, &gdk::ModifierType::empty()],
    );
    assert_eq!(
        state(&w).preferences.query,
        "P",
        "focused search uses native text input"
    );
    for (theme, suffix) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for page in SettingsPage::ALL {
            w.dispatch(UiAction::OpenSettings { page });
            assert!(state(&w).settings_open, "open action must reach the core");
            pump(400);
            assert!(state(&w).settings_open, "settings must remain open");
            assert!(
                w.preferences.dialog.is_mapped(),
                "open settings must map the native dialog"
            );
            let search_toggle: gtk::ToggleButton = find_named(
                w.preferences.dialog.upcast_ref(),
                "preferences-search-toggle",
            )
            .unwrap()
            .downcast()
            .unwrap();
            assert_eq!(
                search_toggle.icon_name().as_deref(),
                Some("edit-find-symbolic")
            );
            let sidebar =
                find_named(w.preferences.dialog.upcast_ref(), "preferences-sidebar").unwrap();
            let sidebar_bounds = sidebar.compute_bounds(&w.window).unwrap();
            let content = find_named(w.preferences.dialog.upcast_ref(), "preferences-content")
                .unwrap()
                .compute_bounds(&w.window)
                .unwrap();
            capture_reference(&w, &format!("{dir}/gtk-{}-{suffix}.png", page.key()), 1.0);
            assert!(
                (sidebar_bounds.y() + sidebar_bounds.height() - content.y() - content.height())
                    .abs()
                    < 1.0,
                "sidebar and settings content finish at the same height"
            );
            assert_eq!(
                ui_session(&w)
                    .preferences()
                    .unwrap()
                    .page,
                page
            );
            if page == SettingsPage::Input {
                let prediction: crate::number_control::NumberControl = find_named(
                    w.preferences.dialog.upcast_ref(),
                    "setting-prediction-horizon",
                )
                .unwrap()
                .downcast()
                .unwrap();
                assert_eq!(prediction.value(), 16.0);
                edit_number(&prediction, "32");
                assert_eq!(state(&w).settings.prediction_ms, 32.0);
                let display: gtk::Button = find_css(prediction.upcast_ref(), "number-value")
                    .unwrap()
                    .downcast()
                    .unwrap();
                click(&display);
                let entry: gtk::Entry = find_css(prediction.upcast_ref(), "number-entry")
                    .unwrap()
                    .downcast()
                    .unwrap();
                entry.set_text("");
                assert_eq!(
                    state(&w).settings.prediction_ms,
                    32.0,
                    "empty numeric draft does not reset"
                );
                entry.emit_activate();
                assert_eq!(state(&w).settings.prediction_ms, 16.0);
                assert_eq!(prediction.value(), 16.0);
                let field =
                    find_named(w.preferences.dialog.upcast_ref(), "setting-pressure").unwrap();
                let title = find_css(&field, "number-title").unwrap();
                let feedback =
                    find_named(w.preferences.dialog.upcast_ref(), "setting-feedback").unwrap();
                let native_title = find_css(&feedback, "title").unwrap();
                assert_eq!(
                    title.compute_bounds(&w.window).unwrap().x(),
                    native_title.compute_bounds(&w.window).unwrap().x(),
                    "slider labels align with native settings rows"
                );
                assert_eq!(
                    title.compute_bounds(&field).unwrap().x(),
                    0.0,
                    "only panel slider labels get an extra inset"
                );
                let scale = field
                    .last_child()
                    .unwrap()
                    .first_child()
                    .unwrap()
                    .next_sibling()
                    .unwrap()
                    .downcast::<gtk::Scale>()
                    .unwrap();
                assert_eq!(scale.height(), 32, "settings keep the expanded slider");
                let (start, end) = scale.slider_range();
                assert!(end - start >= 16, "settings keep the visible thumb");
            }
            if page == SettingsPage::About {
                assert!(w.preferences.dialog.is_mapped());
                let view = ui_session(&w)
                    .preferences()
                    .unwrap();
                for row in view
                    .pages
                    .iter()
                    .flat_map(|p| &p.groups)
                    .flat_map(|g| &g.rows)
                {
                    if let PreferenceKind::Link { label, url } = &row.kind {
                        let native: adw::ActionRow = find_named(
                            w.preferences.dialog.upcast_ref(),
                            &format!("setting-{}", row.id.key()),
                        )
                        .unwrap()
                        .downcast()
                        .unwrap();
                        let link = native
                            .activatable_widget()
                            .unwrap()
                            .downcast::<gtk::LinkButton>()
                            .unwrap();
                        assert_eq!(native.title().as_str(), row.title);
                        assert_eq!(link.uri().as_str(), url);
                        assert_eq!(link.label().as_deref(), Some(label.as_str()));
                        // Exercise activation without opening the user's browser.
                        let activated = Rc::new(std::cell::Cell::new(false));
                        let seen = activated.clone();
                        let handler = link.connect_activate_link(move |_| {
                            seen.set(true);
                            glib::Propagation::Stop
                        });
                        link.emit_clicked();
                        assert!(activated.get());
                        link.disconnect(handler);
                    }
                }
            }
        }
        w.dispatch(UiAction::CloseSettings);
        pump(250);
    }
    for (theme, id, color) in [
        (Theme::Dark, PreferenceId::DarkBase, "#1C2C3C"),
        (Theme::Light, PreferenceId::LightBase, "#C0B49C"),
    ] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::OpenSettings {
            page: SettingsPage::Appearance,
        });
        pump(300);
        let entry: gtk::Entry = find_named(
            w.preferences.dialog.upcast_ref(),
            &format!("setting-text-{}", id.key()),
        )
        .unwrap()
        .downcast()
        .unwrap();
        entry.grab_focus();
        entry.set_text("invalid");
        entry.emit_activate();
        assert!(state(&w).preferences.error.is_some());
        entry.set_text(color);
        entry.emit_activate();
        pump(250);
        assert!(state(&w).preferences.error.is_none());
        assert_eq!(state(&w).palette.bg.to_string(), color.to_lowercase());
        let suffix = if theme == Theme::Dark {
            "dark"
        } else {
            "light"
        };
        capture_reference(&w, &format!("{dir}/gtk-custom-base-{suffix}.png"), 1.0);
        let row = find_named(
            w.preferences.dialog.upcast_ref(),
            &format!("setting-{}", id.key()),
        )
        .unwrap();
        let controllers = row.observe_controllers();
        let secondary = (0..controllers.n_items())
            .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
            .find(|c| c.name().as_deref() == Some("preference-context-click"))
            .unwrap();
        secondary.emit_by_name::<()>("pressed", &[&1i32, &20.0f64, &20.0f64]);
        pump(100);
        let popup: gtk::PopoverMenu =
            find_named(w.preferences.dialog.upcast_ref(), "preference-context-menu")
                .unwrap()
                .downcast()
                .unwrap();
        assert!(
            popup.is_visible(),
            "reset menu must survive the editor losing focus"
        );
        let reset_label = format!("Reset to Default ({})", theme.default_base());
        let reset = find_menu_item(popup.upcast_ref(), &reset_label).unwrap();
        assert!(reset.is_sensitive());
        capture_popover(popup.upcast_ref(), &format!("{dir}/gtk-reset-{suffix}.png"));
        popup.activate_action("field.reset", None).unwrap();
        assert_eq!(state(&w).palette.bg, theme.default_base());
        secondary.emit_by_name::<()>("pressed", &[&1i32, &20.0f64, &20.0f64]);
        pump(100);
        let reset = find_menu_item(popup.upcast_ref(), &reset_label).unwrap();
        assert!(!reset.is_sensitive());
        popup.popdown();
        entry.grab_focus();
        entry.set_text(color);
        entry.emit_activate();
        entry.set_text("");
        assert_eq!(
            state(&w).palette.bg.to_string(),
            color.to_lowercase(),
            "empty draft does not reset yet"
        );
        entry.emit_activate();
        assert_eq!(entry.text(), theme.default_base().to_string());
        assert_eq!(state(&w).palette.bg, theme.default_base());
        entry.set_text(color);
        entry.emit_activate();
        w.dispatch(UiAction::CloseSettings);
        pump(300);
        capture_reference(&w, &format!("{dir}/gtk-custom-workspace-{suffix}.png"), 1.0);
    }
    let mut defaults = state(&w).settings;
    defaults.dark_base = Theme::Dark.default_base();
    defaults.light_base = Theme::Light.default_base();
    w.dispatch(UiAction::RestoreSettings { settings: defaults });
    click(&command(&w, CommandId::KeyboardShortcuts));
    let search: gtk::SearchEntry = find_named(w.preferences.dialog.upcast_ref(), "settings-search")
        .unwrap()
        .downcast()
        .unwrap();
    search.set_text("pressure response");
    pump(300);
    let results = ui_session(&w)
        .preferences()
        .unwrap()
        .search_results;
    assert_eq!(results.len(), 1);
    w.dispatch(UiAction::Preferences {
        action: results[0].action.clone(),
    });
    search.set_text("");
    pump(300);
    let feedback: adw::SwitchRow =
        find_named(w.preferences.dialog.upcast_ref(), "setting-feedback")
            .unwrap()
            .downcast()
            .unwrap();
    feedback.set_active(false);
    assert!(
        !find_named(
            w.preferences.dialog.upcast_ref(),
            "setting-prediction-horizon"
        )
        .unwrap()
        .is_sensitive()
    );
    feedback.set_active(true);
    let prediction = named::<adw::SwitchRow>(w.preferences.dialog.upcast_ref(), "setting-platform-prediction");
    assert!(!prediction.is_sensitive());
    assert!(prediction.subtitle().is_none_or(|subtitle| subtitle.is_empty()));
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Shortcuts,
    });
    let search: gtk::SearchEntry =
        find_named(w.preferences.dialog.upcast_ref(), "shortcuts-search")
            .unwrap()
            .downcast()
            .unwrap();
    search.set_text("z");
    pump(300);
    for command in ["Undo", "Redo", "UndoWorkspace", "RedoWorkspace"] {
        assert!(
            find_named(
                w.preferences.dialog.upcast_ref(),
                &format!("shortcut-command.{command}")
            )
            .unwrap()
            .is_visible()
        );
    }
    search.set_text("");
    pump(300);
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::ShortcutCategory { id: Some("Tools".into()) },
    });
    pump(300);
    // Hand has a direct default; Brush's B now belongs to its cycling family.
    let row: adw::ActionRow =
        find_named(w.preferences.dialog.upcast_ref(), "shortcut-command.Hand")
            .unwrap()
            .downcast()
            .unwrap();
    let binding = find_named(row.upcast_ref(), "shortcut-reset-command.Hand").unwrap();
    assert!(!binding.is_visible());
    row.emit_by_name::<()>("activated", &[]);
    pump(200);
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::RemoveShortcut {
            id: CommandId::Hand.shortcut_id(),
            index: 0,
        },
    });
    assert!(binding.is_visible());
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::ResetShortcut {
            id: CommandId::Hand.shortcut_id(),
        },
    });
    assert!(!binding.is_visible());
    find_named(w.window.upcast_ref(), "add-shortcut")
        .unwrap()
        .emit_by_name::<()>("activated", &[]);
    pump(200);
    assert!(find_named(w.window.upcast_ref(), "shortcut-recording").unwrap().is_mapped());
    let controllers = find_named(w.window.upcast_ref(), "shortcut-editor").unwrap().observe_controllers();
    let keys = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::EventControllerKey>())
        .unwrap();
    assert!(keys.emit_by_name::<bool>(
        "key-pressed",
        &[&gdk::Key::e, &0u32, &gdk::ModifierType::empty()]
    ));
    keys.emit_by_name::<()>(
        "key-released",
        &[&gdk::Key::e, &0u32, &gdk::ModifierType::empty()],
    );
    assert_eq!(
        state(&w).preferences.capture.unwrap().conflict.as_deref(),
        Some("Eraser")
    );
    pump(200);
    capture_reference(&w, &format!("{dir}/gtk-shortcut-conflict.png"), 1.0);
    let confirm: gtk::Button = find_named(w.window.upcast_ref(), "confirm-shortcut")
        .unwrap()
        .downcast()
        .unwrap();
    click(&confirm);
    assert!(state(&w).preferences.capture.is_none());
    assert_eq!(state(&w).settings.shortcuts["command.Hand"][1].key, "e");
    assert!(binding.is_visible());
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::CloseShortcutEditor,
    });
    pump(250);
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(400);
        assert_eq!(
            adw::StyleManager::for_display(&w.area.display()).is_dark(),
            theme == Theme::Dark
        );
        assert!(binding.is_mapped(), "a changed shortcut offers its reset button");
        capture_reference(
            &w,
            &format!("{dir}/gtk-shortcut-modified-{theme:?}.png"),
            1.0,
        );
    }
    click(
        &find_css(
            &find_named(w.preferences.dialog.upcast_ref(), "preferences-content").unwrap(),
            "close",
        )
        .unwrap()
        .downcast()
        .unwrap(),
    );
    assert!(
        state(&w).requests.is_empty(),
        "host acknowledged the saved snapshot"
    );
    assert!(
        ui_session(&w)
            .application_menu(ApplicationMenu::Edit)
            .sections
            .iter()
            .flatten()
            .any(|item| item.action
                == Some(UiAction::Invoke {
                    command: CommandId::Settings
                })
                && item.hint == "Ctrl+,")
    );
    // Explicit override isolates persistence from the user's actual config.
    if crate::storage::roots().is_some() {
        assert_eq!(
            crate::preferences::load().unwrap().unwrap(),
            state(&w).settings
        );
        click(&command(&w, CommandId::NewWindow));
        until(|| windows.borrow().len() == 2 && windows.borrow()[1].window.is_mapped(), "persisted preferences window mapped");
        let next = windows.borrow().last().unwrap().clone();
        assert_eq!(state(&next).settings, state(&w).settings);
        next.window.destroy();
        pump(100);
        w.window.present();
    }
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Canvas,
    });
    w.window.set_default_size(640, 600);
    pump(500);
    capture_reference(&w, &format!("{dir}/gtk-narrow.png"), 1.0);
    w.window.destroy();
    pump(200);
    assert!(windows.borrow().is_empty());
}

#[test]
#[ignore = "native slider feedback: requires a Wayland/Vulkan display"]
fn native_slider_feedback() {
    let app = native_test_app("dev.layer.SliderFeedbackTest");
    let slider = |control: &crate::number_control::NumberControl| {
        control
            .last_child()
            .unwrap()
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Scale>()
            .unwrap()
    };
    for spec in [
        NumericControl::brush_size(),
        NumericControl::percent(),
        NumericControl::pressure(),
    ] {
        let control = crate::number_control::NumberControl::new(spec.clone(), "Value", "", layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
        let scale = slider(&control);
        let notifications = Rc::new(RefCell::new(Vec::new()));
        control.connect_value_changed({
            let notifications = notifications.clone();
            move |field| {
                let mut values = notifications.borrow_mut();
                values.push(field.value());
                // End a broken feedback loop at an exactly representable value,
                // so the regression fails instead of hanging the test process.
                let echo = if values.len() > 8 {
                    1.0
                } else {
                    field.value() as f32 as f64
                };
                drop(values);
                field.set_value(echo);
            }
        });
        control.set_value(0.6);
        assert!(
            notifications.borrow().is_empty(),
            "model refresh is not a user edit"
        );
        for i in 1..100 {
            notifications.borrow_mut().clear();
            let position = i as f64 / 100.0;
            scale.set_value(position);
            assert!(
                notifications.borrow().len() <= 1,
                "feedback at {position}: {:?}",
                notifications.borrow()
            );
            let expected = spec
                .resolve(0.0, NumericOperation::Position { position })
                .unwrap()
                .value as f32;
            assert_eq!(control.value() as f32, expected);
        }
    }
    // Exercise the real session/refresh path, including fractional f32 echoes,
    // in both directions while allowing GTK's event loop to advance.
    let w = fixture_workspace(&app);
    w.window.present();
    pump(600);
    let spec = NumericControl::brush_size();
    let scale = slider(&w.size_number);
    let edits = Rc::new(Cell::new(0));
    w.size_number.connect_value_changed({
        let edits = edits.clone();
        move |_| edits.set(edits.get() + 1)
    });
    for i in (0..=200).chain((0..200).rev()) {
        edits.set(0);
        let position = i as f64 / 200.0;
        scale.emit_by_name::<bool>("change-value", &[&gtk::ScrollType::Jump, &position]);
        let expected = spec
            .resolve(0.0, NumericOperation::Position { position })
            .unwrap()
            .value as f32;
        assert_eq!(state(&w).brush.diameter, expected);
        assert!(edits.get() <= 1, "one user action must not feed back");
        if i % 10 == 0 {
            pump(1);
        }
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "numeric widget editing and review sheet: requires a Wayland display"]
fn native_number_controls() {
    let app = native_test_app("dev.layer.NumberTest");
    gtk::gio::resources_register_include!("layer-icons.gresource").unwrap();
    gtk::IconTheme::for_display(&gdk::Display::default().unwrap())
        .add_resource_path("/dev/layer/icons");
    let window = adw::ApplicationWindow::builder()
        .application(&*app)
        .default_width(560)
        .default_height(360)
        .build();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(18);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);
    body.add_css_class("dock-panel");
    let size =
        crate::number_control::NumberControl::new(NumericControl::brush_size(), "Brush size", "", layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
    let alpha = crate::number_control::NumberControl::new(NumericControl::percent(), "Opacity", "", layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
    let small = crate::number_control::NumberControl::new(
        NumericControl::number(0.0, 16.0, 1.0, 0),
        "Small integer",
        "",
     layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
    for field in [&size, &alpha, &small] {
        body.append(field);
    }
    size.set_value(32.0);
    alpha.set_value(0.5);
    small.set_value(4.0);
    let narrow = crate::number_control::NumberControl::new(
        NumericControl::percent(),
        "A long slider name that must not wrap",
        "",
     layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
    narrow.set_value(0.5);
    let narrow_container = adw::Clamp::builder()
        .maximum_size(176)
        .tightening_threshold(176)
        .child(&narrow)
        .build();
    body.append(&narrow_container);
    let described = crate::number_control::NumberControl::new(
        NumericControl::pressure(),
        "Pressure response",
        "Adjust how pen pressure affects your brush. The value centers against this complete label block.",
     layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
    described.set_value(1.0);
    body.append(&described);
    window.set_content(Some(&body));
    window.present();
    pump(300);
    let scale: gtk::Scale = descendant(&size).unwrap();
    for field in [&size, &narrow, &described] {
        let header = field.first_child().unwrap();
        let labels = header.first_child().unwrap();
        let title = labels
            .first_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let value = header.last_child().unwrap();
        let lb = labels.compute_bounds(field).unwrap();
        let vb = value.compute_bounds(field).unwrap();
        assert!((lb.y() + lb.height() / 2.0 - vb.y() - vb.height() / 2.0).abs() <= 1.0);
        assert!((vb.x() + vb.width() - field.width() as f32).abs() <= 1.0);
        assert!(!title.wraps());
        assert_eq!(title.ellipsize(), gtk::pango::EllipsizeMode::End);
        assert_eq!(title.tooltip_text().unwrap(), title.text());
        assert_eq!(title.compute_bounds(field).unwrap().x(), 6.0);
        if field == &narrow {
            assert!(title.layout().is_ellipsized());
        }
        assert!(
            vb.width() < 90.0,
            "hidden entry must not reserve width: {vb:?}"
        );
    }
    assert!(
        narrow.height() <= 50,
        "compact row height: {}",
        narrow.height()
    );
    assert_eq!(narrow.width(), 176);
    let track = scale.parent().unwrap();
    let minus = track.first_child().unwrap().compute_bounds(&track).unwrap();
    let plus = track.last_child().unwrap().compute_bounds(&track).unwrap();
    let bar = scale.compute_bounds(&track).unwrap();
    assert_eq!(bar.height(), 24.0);
    assert_eq!(bar.x(), minus.x() + minus.width() + 6.0);
    assert_eq!(bar.x() + bar.width() + 6.0, plus.x());
    assert_eq!(scale.range_rect().width(), scale.width());
    let (start, end) = scale.slider_range();
    assert_eq!(start, end, "compact slider reserves no thumb width");
    let value_label: gtk::Label = descendant(&descendant::<gtk::Button>(&size).unwrap()).unwrap();
    assert_eq!(value_label.xalign(), 1.0);
    scale.set_value(0.5);
    assert!((size.value() - 32.0).abs() < 0.1);
    let display: gtk::Button = descendant(&size).unwrap();
    click(&display);
    let entry: gtk::Entry = descendant(&size).unwrap();
    entry.set_text("85/2");
    entry.emit_activate();
    assert_eq!(size.value(), 43.);
    click(&display);
    entry.set_text("1/0");
    entry.emit_activate();
    assert!(size.has_css_class("error"));
    assert_eq!(size.value(), 43.);
    entry.set_text("2049");
    entry.emit_activate();
    assert_eq!(size.value(), 2048.0);
    let spin: gtk::SpinButton = descendant(&small).unwrap();
    spin.set_text("3*2");
    spin.update();
    assert_eq!(small.value(), 6.0);
    spin.set_text("sqrt(81)");
    spin.update();
    assert_eq!(small.value(), 9.0);
    click(&descendant::<gtk::Button>(&alpha).unwrap());
    let percent: gtk::Entry = descendant(&alpha).unwrap();
    percent.set_text("75%");
    percent.emit_activate();
    assert_eq!(alpha.value(), 0.75);
    let dir = artifact_dir("../../artifacts/ui/numeric");
    for (theme, scheme) in [
        ("dark", adw::ColorScheme::ForceDark),
        ("light", adw::ColorScheme::ForceLight),
    ] {
        app.style_manager().set_color_scheme(scheme);
        if theme == "light" {
            window.add_css_class("light-theme");
        }
        pump(150);
        crate::snapshot_window(&window, 1.0)
            .save_to_png(format!("{dir}/gtk-{theme}.png"))
            .unwrap();
        let value: gtk::Button = descendant(&narrow).unwrap();
        click(&value);
        pump(100);
        let input: gtk::Entry = descendant(&narrow).unwrap();
        assert!(input.width() < 100, "short values use compact editors");
        crate::snapshot_window(&window, 1.0)
            .save_to_png(format!("{dir}/gtk-edit-{theme}.png"))
            .unwrap();
        input.emit_activate();
    }
    // Preferences use the native entry height, not the compact panel editor.
    body.remove(&described);
    let preferences = gtk::Box::new(gtk::Orientation::Vertical, 12);
    preferences.add_css_class("number-preference");
    preferences.append(&described);
    let standard = gtk::Entry::builder().text("Standard GTK entry").build();
    preferences.append(&standard);
    window.set_content(Some(&preferences));
    for (theme, scheme) in [
        ("dark", adw::ColorScheme::ForceDark),
        ("light", adw::ColorScheme::ForceLight),
    ] {
        app.style_manager().set_color_scheme(scheme);
        if theme == "light" {
            window.add_css_class("light-theme");
        } else {
            window.remove_css_class("light-theme");
        }
        pump(150);
        let value: gtk::Button = descendant(&described).unwrap();
        assert_eq!(value.height(), standard.height());
        click(&value);
        pump(100);
        let entry: gtk::Entry = descendant(&described).unwrap();
        assert_eq!(entry.height(), standard.height());
        assert_eq!(entry.height(), 34);
        crate::snapshot_window(&window, 1.0)
            .save_to_png(format!("{dir}/gtk-settings-edit-{theme}.png"))
            .unwrap();
        entry.set_text("sqrt(4)");
        entry.emit_activate();
        assert_eq!(described.value(), 2.0);
    }
    window.destroy();
}

#[test]
#[ignore = "cursor vectors: requires a Wayland/Vulkan display"]
fn native_cursor_vectors() {
    let app = native_test_app("dev.layer.CursorTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(1800);
    apply_fixture_theme(&w);
    pump(200);
    std::fs::create_dir_all("../../artifacts/ui/cursors").unwrap();
    let scale = w.area.scale_factor() as f32;
    assert_eq!(
        w.area.cursor().and_then(|c| c.name()).as_deref(),
        Some("none")
    );
    let hover = || PenEvent {
        device_id: 123,
        sequence: 1,
        timestamp_ns: 1_000_000_000_000,
        view_revision: state(&w).camera.revision,
        surface_position: Point {
            x: 600.0 * scale,
            y: 460.0 * scale,
        },
        pressure: 0.0,
        tilt_radians: [0.3, 0.6],
        twist_radians: 0.5,
        distance: 0.0,
        phase: PenPhase::Hover,
        tool: ToolKind::Pen,
        flags: SampleFlags::PRIMARY,
    };
    w.dispatch(UiAction::OpenSettings { page: SettingsPage::Input });
    pump(200);
    let toggle = named::<adw::SwitchRow>(w.window.upcast_ref(), "setting-hide-cursor-while-drawing");
    assert!(toggle.is_active());
    toggle.set_active(false);
    assert!(!state(&w).settings.hide_cursor_while_drawing);
    toggle.set_active(true);
    let choice = named::<adw::ComboRow>(w.window.upcast_ref(), "setting-cursor");
    assert_eq!(choice.model().unwrap().n_items(), 12);
    for mode in [layer_ui::CursorMode::Tool, layer_ui::CursorMode::ToolBrushSize] {
        let index = layer_ui::CursorMode::CHOICES.iter().position(|&(item, _)| item == mode).unwrap();
        choice.set_selected(index as u32);
        assert_eq!(state(&w).settings.cursor, mode);
    }
    capture_reference(&w, "../../artifacts/ui/cursors/gtk-input-settings.png", 1.0);
    w.dispatch(UiAction::CloseSettings);
    pump(200);
    w.cursor_input(Some(hover()));
    assert!(ui_session_mut(&w).canvas_cursor().is_some());
    for mode in [layer_ui::CursorMode::Tool, layer_ui::CursorMode::ToolBrushSize] {
        let index = layer_ui::CursorMode::CHOICES.iter().position(|&(item, _)| item == mode).unwrap();
        w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Cursor, value: PreferenceValue::Choice(index as u32) } });
        for command in [CommandId::Pen, CommandId::Pencil, CommandId::Brush, CommandId::Eraser, CommandId::Clone, CommandId::Lasso, CommandId::RectangleSelect] {
            w.dispatch(UiAction::Invoke { command });
            w.cursor_input(Some(hover()));
            pump(150);
            let cursor = ui_session_mut(&w).canvas_cursor().unwrap();
            assert!(cursor.segments.iter().any(|segment| segment.marker >= 7.));
            let theme = state(&w).theme;
            capture_reference(&w, &format!("../../artifacts/ui/cursors/gtk-{theme:?}-{mode:?}-{command:?}.png"), 1.);
        }
    }
    w.cursor_input(None);
    assert!(
        ui_session_mut(&w)
            .canvas_cursor()
            .is_none()
    );
    w.window.destroy();
}

#[test]
#[ignore = "workspace restore integration: requires a Wayland display"]
fn native_workspace_restore() {
    let app = native_test_app("dev.layer.RestoreTest");
    let source = fixture_workspace(&app);
    source.window.present();
    pump(1000);
    source.dispatch(UiAction::MovePanel {
        panel: Panel::Toolbar,
        target: DockTarget::Tab {
            group: 5,
            index: Some(0),
        },
        viewport: [1200.0, 900.0],
    });
    drag_divider(&source, 3, [320.0, 450.0]);
    source.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    pump(150);
    let saved = serde_json::to_string(&state(&source).workspace).unwrap();
    let geometry = |w: &Workspace| {
        w.groups
            .borrow()
            .iter()
            .map(|g| {
                let b = g
                    .stack
                    .parent()
                    .unwrap()
                    .compute_bounds(&w.surface)
                    .unwrap();
                (
                    g.id,
                    g.panels.clone(),
                    g.stack.visible_child_name(),
                    [b.x(), b.y(), b.width(), b.height()],
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = geometry(&source);
    source.window.destroy();
    drop(source);
    pump(100);
    let fresh = Workspace::new(&app);
    fresh.window.present();
    pump(1000);
    fresh.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(serde_json::from_str(&saved).unwrap()),
    });
    pump(150);
    assert_eq!(
        serde_json::to_string(&state(&fresh).workspace).unwrap(),
        saved
    );
    assert_eq!(geometry(&fresh), expected);
    assert!(command(&fresh, CommandId::ZenMode).has_css_class("selected-tool"));
    assert_eq!(
        fresh
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == 5)
            .unwrap()
            .tabs
            .len(),
        2
    );
    assert!(
        !find_css(fresh.toolbar.upcast_ref(), "panel-grip")
            .unwrap()
            .is_child_visible()
    );
    assert!(!fresh.status.is_visible());
    fresh.window.destroy();
    pump(100);
}

pub(crate) fn set_transparency(w: &Rc<Workspace>, variable: &str) {
    let Ok(level) = std::env::var(variable) else { return };
    let level = layer_ui::Transparency::CHOICES
        .iter()
        .find(|(_, label)| label.eq_ignore_ascii_case(&level))
        .expect("off, low, medium or high")
        .0;
    use_transparency(w, level);
}

pub(crate) fn use_transparency(w: &Rc<Workspace>, level: layer_ui::Transparency) {
    let index = layer_ui::Transparency::CHOICES.iter().position(|(l, _)| *l == level).unwrap();
    w.dispatch(UiAction::Preferences {
        action: layer_ui::PreferenceAction::Edit {
            id: layer_ui::PreferenceId::Transparency,
            value: layer_ui::PreferenceValue::Choice(index as u32),
        },
    });
    pump(200);
    assert_eq!(state(w).settings.transparency, level);
}

#[test]
#[ignore = "isolated native-input.js with LAYER_NATIVE_CAPTURE_DIR"]
fn native_backdrop_blur_capture() {
    use layer_core::DefaultBrushPreset;
    use layer_ui::{PreferenceAction, PreferenceId, PreferenceValue, WorkspacePreset};
    let (app, windows) = crate::application("art.capycanvas.BackdropBlur");
    let app = NativeTestApp(app);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    crate::open_workspace(&app, &windows, None);
    until(|| windows.borrow().first().is_some_and(|workspace| workspace.window.is_mapped()), "backdrop window mapped");
    let w = windows.borrow()[0].clone();
    w.window.maximize();
    w.window.present();
    pump(1500);
    until(
        || {
            w.gpu
                .borrow()
                .as_ref()
                .is_some_and(|g| g.session.engine().backend().startup.complete)
        },
        "startup",
    );
    if std::env::var("LAYER_GLASS_THEME").as_deref() == Ok("light") {
        w.dispatch(UiAction::SetTheme { theme: Some(layer_ui::Theme::Light) });
    }
    let level = match std::env::var("LAYER_GLASS_LEVEL").as_deref() {
        Ok("off") => 0,
        Ok("low") => 1,
        Ok("high") => 3,
        _ => 2,
    };
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::Edit { id: PreferenceId::Transparency, value: PreferenceValue::Choice(level) },
    });
    assert_eq!(state(&w).settings.transparency, layer_ui::Transparency::CHOICES[level as usize].0);
    if std::env::var("LAYER_GLASS_DOCUMENTS").as_deref() != Ok("0") {
        new_photo::invoke(&w, CommandId::NewDocument);
        new_photo::response(&w, "create");
        until(
            || w.documents.len() == 2 && !w.documents.changing.get(),
            "second document",
        );
        new_photo::ready(&w);
        pump(300);
    }
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    for _ in 0..std::env::var("LAYER_GLASS_ZOOM").map_or(3, |v| v.parse().unwrap()) {
        w.dispatch(UiAction::Invoke { command: CommandId::ZoomIn });
    }
    pump(300);
    w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 });
    w.dispatch(UiAction::SetBrushSize { value: 90. });
    let camera = state(&w).camera;
    let [a, b, c, d, e, f] = camera.document_to_surface();
    let det = a * d - b * c;
    let document = |x: f32, y: f32| {
        let (x, y) = (x - e, y - f);
        [(d * x - c * y) / det, (a * y - b * x) / det]
    };
    let [width, height] = [w.surface.width() as f32, w.surface.height() as f32];
    let colors = [
        [0.9, 0.1, 0.1, 1.], [0.95, 0.75, 0.05, 1.], [0.1, 0.6, 0.2, 1.],
        [0.05, 0.3, 0.9, 1.], [0.6, 0.1, 0.8, 1.], [0.95, 0.95, 0.95, 1.],
    ];
    let probe = std::env::var("LAYER_GLASS_PROBE").as_deref() == Ok("1");
    for (i, color) in colors.iter().enumerate().filter(|_| !probe) {
        w.dispatch(UiAction::SetColor { rgba: *color });
        let y = height * (i as f32 + 0.5) / colors.len() as f32;
        let points: Vec<_> = (0..=24)
            .map(|k| document(width * k as f32 / 24., y + 40. * (k as f32 * 0.7 + i as f32).sin()))
            .collect();
        native_pen_path(&w, &points);
    }
    for (i, color) in colors.iter().rev().enumerate().take(if probe { 0 } else { 3 }) {
        w.dispatch(UiAction::SetColor { rgba: *color });
        let x = width * (i as f32 + 0.5) / 3.;
        let points: Vec<_> = (0..=16).map(|k| document(x + 200. * (k as f32 / 16. - 0.5), height * k as f32 / 16.)).collect();
        native_pen_path(&w, &points);
    }
    pump(500);
    let input = std::env::var_os("LAYER_NATIVE_CAPTURE_DIR")
        .map(|_| RefCell::new(RemoteInput::new().settle_ms(0).timeout_secs(30)));
    if let Some(input) = &input {
        input.borrow().ready();
    }
    let capture = |name: &str| {
        pump(300);
        if std::env::var("LAYER_GLASS_REDRAW").as_deref() == Ok("1") {
            w.window.queue_draw();
            pump(300);
        }
        eprintln!("{name}: {} glass regions", w.glass.borrow().len());
        let Some(input) = &input else { return };
        input
            .borrow_mut()
            .perform(serde_json::json!([{"wait_ms":400},{"capture":name}]));
    };
    if probe {
        fn find(root: &gtk::Widget, test: &dyn Fn(&gtk::Widget) -> bool) -> Option<gtk::Widget> {
            if test(root) && root.is_mapped() {
                return Some(root.clone());
            }
            let mut child = root.first_child();
            while let Some(c) = child {
                if let Some(found) = find(&c, test) {
                    return Some(found);
                }
                child = c.next_sibling();
            }
            None
        }
        let elements = |scene: &str| {
            let root: gtk::Widget = w.window.clone().upcast();
            let selected_doc = format!("document-tab-{}", w.documents.selected());
            let probes: [(&str, &dyn Fn(&gtk::Widget) -> bool); 14] = [
                ("chip", &|x| x.has_css_class("header-menu-labels")),
                ("status", &|x| x.has_css_class("status-bubble")),
                ("doc-selected", &|x| x.widget_name() == selected_doc.as_str()),
                ("doc-other", &|x| x.widget_name().starts_with("document-tab-") && x.widget_name() != selected_doc.as_str() && !x.widget_name().contains('s')),
                ("switcher", &|x| x.widget_name() == "workspace-switcher"),
                ("switcher-selected", &|x| x.downcast_ref::<gtk::ToggleButton>().is_some_and(|b| b.is_active()) && x.ancestor(gtk::Widget::static_type()).is_some() && x.parent().is_some_and(|p| p.parent().is_some_and(|q| q.widget_name() == "workspace-switcher" || q.parent().is_some_and(|r| r.widget_name() == "workspace-switcher")))),
                ("panel", &|x| x.has_css_class("layers-panel")),
                ("strip", &|x| x.has_css_class("dock-tabs")),
                ("layer-selected", &|x| x.has_css_class("layer-row") && x.has_css_class("selected")),
                ("connector", &|x| x.widget_name().starts_with("column-connection-") || x.widget_name() == "drawer-connection"),
                ("header-bar", &|x| x.has_css_class("header-bar")),
                ("header-selected", &|x| x.has_css_class("header-tool") && x.has_css_class("selected-tool")),
                ("drawer", &|x| x.widget_name().starts_with("column-drawer-")),
                ("column", &|x| x.has_css_class("collapsed-column")),
            ];
            for (name, test) in probes {
                if let Some(widget) = find(&root, test)
                    && let Some(b) = widget.compute_bounds(&w.surface)
                {
                    eprintln!("ELEM {scene} {name} {} {} {} {}", b.x(), b.y(), b.width(), b.height());
                }
            }
        };
        let layouts = |backdrop: &str| {
            for (name, preset) in [("paint", None), ("sketch", Some(WorkspacePreset::Painter)), ("photo", Some(WorkspacePreset::Photographer))] {
                w.dispatch(UiAction::RestoreWorkspace {
                    workspace: Box::new(layer_ui::WorkspaceState {
                        layout: preset.unwrap_or(WorkspacePreset::Illustrator).layout(Platform::Gtk),
                        ..layer_ui::WorkspaceState::default()
                    }),
                });
                pump(400);
                if preset == Some(WorkspacePreset::Photographer)
                    && let Some((group, panel)) = w.resolved().collapsed.first().and_then(|c| c.groups.first()).map(|g| (g.group, g.active))
                {
                    w.dispatch(UiAction::Customize { action: CustomizationAction::ToggleColumnDrawer { group, panel } });
                    pump(300);
                }
                let scene = format!("{name}-{backdrop}");
                elements(&scene);
                capture(&scene);
            }
        };
        layouts("white");
        w.dispatch(UiAction::SetColor { rgba: [0.2, 0.2, 0.2, 1.] });
        w.dispatch(UiAction::SetBrushSize { value: 1000. });
        let rows = 8;
        let points: Vec<_> = (0..=rows)
            .flat_map(|r| {
                let y = height * r as f32 / rows as f32;
                (0..=12).map(move |k| {
                    let t = k as f32 / 12.;
                    [if r % 2 == 0 { t } else { 1. - t } * width, y]
                })
            })
            .map(|[x, y]| document(x, y))
            .collect();
        native_pen_path(&w, &points);
        pump(300);
        layouts("grey");
        w.window.destroy();
        pump(100);
        return;
    }
    capture("paint");
    for (name, preset) in [("sketch", WorkspacePreset::Painter), ("photo", WorkspacePreset::Photographer)] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState {
                layout: preset.layout(Platform::Gtk),
                ..layer_ui::WorkspaceState::default()
            }),
        });
        pump(400);
        let layout = state(&w).workspace.layout;
        if preset == WorkspacePreset::Painter {
            let ids: Vec<u32> = layout.header.zones.iter().flatten().map(|entry| entry.id).collect();
            for id in ids {
                w.dispatch(UiAction::Customize { action: CustomizationAction::ToggleHeaderDrawer { id } });
                pump(100);
                if state(&w).customization.drawer.is_some() {
                    break;
                }
            }
        } else if let Some(group) = w.resolved().collapsed.first().and_then(|c| c.groups.first()).map(|g| (g.group, g.active)) {
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::ToggleColumnDrawer { group: group.0, panel: group.1 },
            });
        }
        capture(name);
    }
    w.dispatch(UiAction::Preferences { action: PreferenceAction::Reveal { id: PreferenceId::Transparency } });
    pump(600);
    capture("preferences");
    let mut nodes = Vec::new();
    let mut child = w.surface.first_child();
    while let Some(c) = child {
        let snapshot = gtk::Snapshot::new();
        w.surface.snapshot_child(&c, &snapshot);
        nodes.extend(snapshot.to_node());
        child = c.next_sibling();
    }
    let surfaces = state(&w).palette.glass.surfaces();
    let start = Instant::now();
    let mut found = 0;
    for _ in 0..200 {
        let mut regions = Vec::new();
        for node in &nodes {
            crate::glass::collect(node, &surfaces, &mut regions);
        }
        found = regions.len();
    }
    eprintln!("glass walk: {found} regions, {:.1} us per snapshot", start.elapsed().as_secs_f64() * 1e6 / 200.);
    w.window.destroy();
    pump(100);
}

fn refine_busy(w: &Workspace) -> bool {
    ui_session(&w).wants_continuous_frames()
}
fn refine_preview_count(w: &Workspace) -> u64 {
    ui_session(&w).renderer_stats().selection_previews
}
fn finish_canvas_operation(w: &Rc<Workspace>) {
    let open = || matches!(state(w).layer_tools.tool, LayerCanvasTool::Transform | LayerCanvasTool::Crop);
    until(
        || {
            if open() {
                w.dispatch(UiAction::Invoke { command: CommandId::CancelTransform });
            }
            !open() && state(w).layer_tools.selection_resize.is_none()
        },
        "the workload leaves no transform, crop or refinement open",
    );
}

#[test]
#[ignore = "hardware Wayland benchmark: run separately in release with --ignored --test-threads=1"]
fn native_frame_pacing() {
    use layer_core::DefaultBrushPreset;
    let app = native_test_app("dev.layer.FramePacingTest");
    let w = match std::env::var("LAYER_PACING_WORKSPACE").as_deref() {
        Ok("fixture") => fixture_workspace(&app),
        Ok("photo24") => {
            let mut project = native_navigation::photo([6000, 4000]);
            match std::env::var("LAYER_PACING_PHOTO_LAYERS").as_deref() {
                Ok("photo") => {
                    let members = project.scene().order().iter().copied().filter(|h| project.scene().paint_source(*h).is_some_and(|p| p.base.is_some())).collect();
                    let stack = project.composition().result;
                    project.apply(layer_core::Edit::Stack(layer_core::authored::RecordChange::replace(&project.artwork.stacks, stack, Some(layer_core::authored::Stack { entries: members })).unwrap())).unwrap();
                },
                Ok("layered") => native_navigation::layered(&mut project),
                Ok("blended") => native_navigation::blended(&mut project),
                Ok("pass_through") => native_navigation::pass_through(&mut project),
                _ => {}
            }
            let w = Workspace::with_project(&app, Some((project, None)));
            w.window.maximize();
            w
        }
        Ok("default") | Err(_) => Workspace::new(&app),
        Ok(_) => panic!("Unknown pacing workspace"),
    };
    let photo = std::env::var("LAYER_PACING_WORKSPACE").as_deref() == Ok("photo24");
    let brush_size: f32 = std::env::var("LAYER_PACING_BRUSH_SIZE").map_or(384., |v| v.parse().unwrap());
    let distort = std::env::var("LAYER_PACING_TRANSFORM_MODE").as_deref() == Ok("distort");
    let outline = std::env::var("LAYER_PACING_TRANSFORM_MODE").as_deref() == Ok("outline");
    let interpolation = std::env::var("LAYER_PACING_INTERPOLATION").unwrap_or_default();
    w.window.present();
    pump(1500);
    set_transparency(&w, "LAYER_PACING_TRANSPARENCY");
    let rulers = std::env::var("LAYER_PACING_RULER").unwrap_or_default();
    if rulers == "visible" || rulers == "snap" {
        w.dispatch(UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::Ruler {
                    kind: layer_ui::RulerKind::Parallel,
                },
            },
        });
        native_pen_path(&w, &[[600., 600.], [1400., 600.]]);
        if rulers == "visible" {
            w.dispatch(UiAction::Invoke {
                command: CommandId::SnapRulers,
            });
        }
    }
    if std::env::var("LAYER_PACING_FIGURE").as_deref() == Ok("1") {
        w.dispatch(UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::Figure {
                    shape: layer_ui::FigureShape::Ellipse,
                    paint: layer_ui::FigurePaint::Both,
                },
            },
        });
        native_pen_path(&w, &[[160., 128.], [1888., 1408.]]);
        pump(150);
        assert_eq!(
            active_raster_operations(ui_session(&w).engine().document()).len(),
            1
        );
    }
    let selection = std::env::var("LAYER_PACING_SELECTION").unwrap_or_default();
    if selection == "1" || selection == "pixels" {
        let points: Vec<_> = (0..=256)
            .map(|i| {
                let a = i as f32 / 256. * std::f32::consts::TAU;
                [1024. + 1000. * a.cos(), 768. + 740. * a.sin()]
            })
            .collect();
        w.dispatch(UiAction::Invoke {
            command: CommandId::Lasso,
        });
        native_pen_path(&w, &points);
        if selection == "pixels" {
            // Turn a painted enclosure into a real GPU-produced raster region.
            // This includes the live presentation-outline path in pacing tests.
            w.dispatch(UiAction::Layer {
                action: LayerAction::Deselect,
            });
            w.dispatch(UiAction::SelectBrush {
                id: DefaultBrushPreset::GPen as u32,
            });
            w.dispatch(UiAction::SetBrushSize { value: 12. });
            native_pen_path(&w, &points);
            w.dispatch(UiAction::Invoke {
                command: CommandId::AutoSelect,
            });
            native_pen_path(&w, &[[1024., 768.], [1024., 768.]]);
            pump(300);
            assert!(matches!(
                ui_session(&w)
                    .engine()
                    .document()
                    .working.selection
                    .as_ref()
                    .unwrap()
                    .shape,
                layer_core::SelectionShape::Pixels(_)
            ));
        }
    }
    assert!(
        w.gpu.borrow().is_some(),
        "hardware Vulkan canvas must initialize"
    );
    let bounds = w.area.compute_bounds(&w.surface).unwrap();
    assert_eq!(
        [bounds.x(), bounds.y(), bounds.width(), bounds.height()],
        [
            0.0,
            0.0,
            w.surface.width() as f32,
            w.surface.height() as f32
        ]
    );
    let worker_stats = ui_session(&w)
        .engine()
        .backend()
        .stats
        .clone();
    let mut reports = Vec::new();
    let mut sequence = 0;
    for (name, preset) in [
        ("GPen", Some(DefaultBrushPreset::GPen)),
        ("Pan", None),
        ("Hand", None),
        ("Transform", None),
        ("Move", None),
        ("Refine", None),
        ("Crop", None),
        ("Clone", Some(DefaultBrushPreset::CloneStamp)),
    ] {
        let chosen = std::env::var("LAYER_PACING_BRUSH");
        if chosen.as_ref().is_ok_and(|s| s != name) || (matches!(name, "Refine" | "Move" | "Clone") && chosen.is_err()) {
            continue;
        }
        if name == "Clone" {
            w.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } });
            w.dispatch(UiAction::Invoke { command: CommandId::UseReferenceBelow });
        }
        if let Some(preset) = preset {
            w.dispatch(UiAction::SelectBrush { id: preset as u32 });
            w.dispatch(UiAction::SetBrushSize { value: brush_size });
        } else if name == "Hand" {
            w.dispatch(UiAction::Invoke {
                command: CommandId::Hand,
            });
        } else if name == "Crop" {
            w.dispatch(UiAction::Invoke {
                command: CommandId::FitCanvas,
            });
            w.dispatch(UiAction::Invoke {
                command: CommandId::Crop,
            });
            assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::Crop);
        } else if name == "Transform" {
            let mut workspace = state(&w).workspace;
            workspace
                .layout
                .set_panel_visible(Panel::ToolSettings, true)
                .unwrap();
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(workspace),
            });
            w.dispatch(UiAction::Invoke {
                command: CommandId::FitCanvas,
            });
            if photo {
                w.dispatch(UiAction::Invoke {
                    command: CommandId::SelectAll,
                });
            } else {
                w.dispatch(UiAction::Layer {
                    action: LayerAction::Tool {
                        tool: LayerCanvasTool::Figure {
                            shape: layer_ui::FigureShape::Rectangle,
                            paint: layer_ui::FigurePaint::Fill,
                        },
                    },
                });
                native_pen_path(&w, &[[160., 128.], [1888., 1408.]]);
            }
            let mask_mode = std::env::var("LAYER_PACING_TRANSFORM_MASK").unwrap_or_default();
            if mask_mode == "watercolor" {
                w.dispatch(UiAction::Layer {
                    action: LayerAction::Tool {
                        tool: LayerCanvasTool::Paint,
                    },
                });
                w.dispatch(UiAction::SelectBrush {
                    id: DefaultBrushPreset::WatercolorWash as u32,
                });
                w.dispatch(UiAction::SetBrushSize { value: 600. });
                native_pen_path(&w, &[[900., 650.], [1100., 800.]]);
            }
            if !mask_mode.is_empty() {
                w.dispatch(UiAction::Invoke {
                    command: CommandId::Lasso,
                });
                native_pen_path(
                    &w,
                    &[
                        [160., 128.],
                        [1888., 128.],
                        [1888., 1408.],
                        [160., 1408.],
                        [160., 128.],
                    ],
                );
                let id = layer_ui::occurrence_token(ui_session(&w).engine().document().working.occurrence.unwrap());
                w.dispatch(UiAction::Layer {
                    action: LayerAction::AddMask { id, replace: false },
                });
            }
            w.dispatch(UiAction::Invoke {
                command: if outline { CommandId::TransformSelectionOutline } else { CommandId::ScaleRotate },
            });
            assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::Transform);
            if distort {
                w.dispatch(UiAction::Invoke {
                    command: CommandId::TransformDistort,
                });
            }
            if let Some(command) = [
                ("nearest", CommandId::TransformNearest),
                ("bilinear", CommandId::TransformBilinear),
                ("bicubic", CommandId::TransformBicubic),
            ]
            .into_iter()
            .find_map(|(key, command)| (key == interpolation).then_some(command))
            {
                w.dispatch(UiAction::Invoke { command });
            }
        } else if name == "Move" {
            w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
            let (width, height) = {
                let gpu = w.gpu.borrow();
                let doc = gpu.as_ref().unwrap().session.engine().document();
                (doc.composition().size[0] as f32, doc.composition().size[1] as f32)
            };
            if !photo {
                w.dispatch(UiAction::Layer {
                    action: LayerAction::Tool {
                        tool: LayerCanvasTool::Figure {
                            shape: layer_ui::FigureShape::Rectangle,
                            paint: layer_ui::FigurePaint::Fill,
                        },
                    },
                });
                native_pen_path(&w, &[[width * 0.1, height * 0.1], [width * 0.9, height * 0.9]]);
            }
            if std::env::var("LAYER_PACING_MOVE").as_deref() == Ok("all") {
                w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
            } else {
                let [x0, y0, x1, y1] = [width * 0.25, height * 0.25, width * 0.75, height * 0.75];
                w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
                native_pen_path(&w, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]);
            }
            if std::env::var("LAYER_PACING_LEAVE_COPY").as_deref() == Ok("1") {
                w.dispatch(UiAction::Invoke { command: CommandId::MoveLeaveCopy });
            }
            w.dispatch(UiAction::Invoke { command: CommandId::Move });
        } else if name == "Refine" {
            w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
            let (width, height) = {
                let gpu = w.gpu.borrow();
                let doc = gpu.as_ref().unwrap().session.engine().document();
                (doc.composition().size[0] as f32, doc.composition().size[1] as f32)
            };
            let [x0, y0, x1, y1] = [width * 0.25, height * 0.25, width * 0.75, height * 0.75];
            w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
            native_pen_path(&w, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]);
            w.dispatch(UiAction::Invoke { command: CommandId::FeatherSelection });
            let deadline = Instant::now() + Duration::from_secs(20);
            while state(&w).layer_tools.selection_resize.is_none() || refine_busy(&w) {
                assert!(Instant::now() < deadline, "the Refine panel opens and previews");
                pump(20);
            }
            pump(500);
        }
        pump(300);
        // A cold material shader/texture may not be ready after a fixed sleep.
        // Starting then suppresses the whole contact and benchmarks an empty
        // canvas. Wait for the actual input gate, and verify committed ink.
        if preset.is_some() {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                let ready = w.gpu.borrow().as_ref().is_some_and(|g| {
                    let engine = g.session.engine();
                    engine.backend().paint_ready(
                        engine.document(),
                        engine.configured_brush(),
                        false,
                    )
                });
                if ready {
                    break;
                }
                assert!(Instant::now() < deadline, "brush did not become ready");
                pump(5);
            }
        }
        // This benchmark measures steady-state presentation. Cold/background
        // compilation is covered separately by native_startup_latency.
        let startup_wait = Instant::now();
        while !ui_session(&w)
            .engine()
            .backend()
            .startup
            .complete
        {
            assert!(
                startup_wait.elapsed() < Duration::from_secs(20),
                "startup did not complete"
            );
            pump(5);
        }
        let startup_wait_ms = startup_wait.elapsed().as_secs_f64() * 1000.;
        let navigator_visible = w.navigator.root.is_mapped()
            && w.navigator.root.opacity() > 0.
            && !w.header.root.has_css_class("zen-hidden");
        let clock = w.area.frame_clock().unwrap();
        let paint_start = Rc::new(Cell::new(None::<Instant>));
        let paint_cpu = Rc::new(RefCell::new(Vec::with_capacity(800)));
        let before_paint = clock.connect_before_paint(glib::clone!(
            #[strong]
            paint_start,
            move |_| paint_start.set(Some(Instant::now()))
        ));
        let after_paint = clock.connect_after_paint(glib::clone!(
            #[strong]
            paint_start,
            #[strong]
            paint_cpu,
            move |_| {
                if let Some(start) = paint_start.take() {
                    paint_cpu
                        .borrow_mut()
                        .push(start.elapsed().as_secs_f64() * 1000.);
                }
            }
        ));
        let mut dispatch_thread_cpu = Vec::with_capacity(4096);
        let strokes_before = ui_session(&w)
            .engine()
            .metrics()
            .committed_strokes;
        *worker_stats.lock().unwrap() = Default::default();
        let camera = state(&w).camera;
        let selection_before = ui_session(&w).engine().document().working.selection.clone();
        let transform_path = match name {
            "Transform" | "Move" => {
                let [x0, y0, x1, y1] = state(&w).canvas_bar.and_then(|b| b.anchor).expect("transform box or selection");
                let from = if distort { [x0, y0] } else { [(x0 + x1) * 0.5, (y0 + y1) * 0.5] };
                Some((from, [(x1 - x0) * 0.17, (y1 - y0) * 0.13]))
            }
            "Crop" => {
                let document = ui_session(&w).engine().document().clone();
                let [width, height] = [document.composition().size[0] as f32, document.composition().size[1] as f32];
                Some(([width, height], [width * 0.17, height * 0.13]))
            }
            _ => None,
        };
        let start = Instant::now();
        let mut first = true;
        let mut last_event = None;
        let first_sequence = sequence;
        let sample_due = Rc::new(Cell::new(false));
        let input_tick = glib::timeout_add_local(
            Duration::from_millis(2),
            glib::clone!(
                #[strong]
                sample_due,
                move || {
                    sample_due.set(true);
                    glib::ControlFlow::Continue
                }
            ),
        );
        let context = glib::MainContext::default();
        let mut refine_values = 0;
        let mut refine_revisions = 0;
        let mut refine_radius = f32::NAN;
        let mut refine_revision = ui_session(&w).engine().document().revision;
        let refine_previews = refine_preview_count(&w);
        while start.elapsed() < Duration::from_secs(6) {
            if name == "Refine" {
                let t = start.elapsed().as_secs_f32();
                let radius = ((2. + 28. * (1. - (std::f32::consts::PI * t).cos())) * 10.).round() / 10.;
                if radius != refine_radius {
                    refine_radius = radius;
                    refine_values += 1;
                    w.dispatch(UiAction::Selection { action: SelectionAction::ResizeRadius { radius } });
                }
                let revision = ui_session(&w).engine().document().revision;
                if revision != refine_revision {
                    refine_revision = revision;
                    refine_revisions += 1;
                }
                let due = start.elapsed() + Duration::from_micros(8333);
                while start.elapsed() < due {
                    let dispatch_start = crate::timing::thread_cpu_ms();
                    context.iteration(true);
                    dispatch_thread_cpu.push(crate::timing::thread_cpu_ms() - dispatch_start);
                }
                sequence += 1;
                continue;
            }
            let t = start.elapsed().as_secs_f32() * 5.0;
            let mut event = PenEvent {
                device_id: 1,
                sequence,
                timestamp_ns: glib::monotonic_time() as u64 * 1000,
                view_revision: camera.revision,
                surface_position: Point {
                    x: camera.viewport[0] as f32 * (0.5 + 0.23 * t.cos()),
                    y: camera.viewport[1] as f32 * (0.5 + 0.20 * (t * 1.3).sin()),
                },
                pressure: 0.8,
                tilt_radians: [0.0; 2],
                twist_radians: 0.0,
                distance: 0.0,
                phase: if first {
                    PenPhase::Down
                } else {
                    PenPhase::Move
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            };
            sequence += 1;
            if let Some((from, reach)) = transform_path {
                let m = camera.document_to_surface();
                let (x, y) = (from[0] + reach[0] * t.sin(), from[1] + reach[1] * (t * 1.3).sin());
                event.surface_position = Point {
                    x: m[0] * x + m[2] * y + m[4],
                    y: m[1] * x + m[3] * y + m[5],
                };
            }
            if preset.is_some() || transform_path.is_some() {
                if preset.is_some() {
                    w.cursor_input(Some(event));
                }
                w.input.send(&w, event);
            } else {
                w.interact(UiInput::Pointer {
                    id: 55,
                    phase: if first {
                        ContactPhase::Down
                    } else {
                        ContactPhase::Move
                    },
                    kind: PointerKind::Mouse,
                    button: if name == "Hand" {
                        PointerButton::Primary
                    } else {
                        PointerButton::Pan
                    },
                    position: [event.surface_position.x, event.surface_position.y],
                    time_ns: 0,
                });
            }
            first = false;
            last_event = Some(event);
            // Wait like the application: any frame/GDK/worker event can wake
            // GLib immediately. Sleeping this thread between synthetic samples
            // would add artificial frame-timer latency. Thread CPU excludes
            // the event wait while including GTK paint and native callbacks.
            while !sample_due.replace(false) {
                let dispatch_start = crate::timing::thread_cpu_ms();
                context.iteration(true);
                dispatch_thread_cpu.push(crate::timing::thread_cpu_ms() - dispatch_start);
            }
        }
        input_tick.remove();
        let input_seconds = start.elapsed().as_secs_f64();
        let input_events = sequence - first_sequence;
        clock.disconnect(before_paint);
        clock.disconnect(after_paint);
        let preview_updates = worker_stats.lock().unwrap().overview_revisions.len();
        if navigator_visible && preset.is_some() {
            assert!(
                preview_updates >= 20,
                "benchmark must keep the overview live"
            );
        }
        if name == "Refine" {
            w.dispatch(UiAction::Selection { action: SelectionAction::CancelResize });
        } else if preset.is_some() || transform_path.is_some() {
            w.input.send(
                &w,
                PenEvent {
                    phase: PenPhase::Up,
                    ..last_event.unwrap()
                },
            );
        } else {
            w.interact(UiInput::Pointer {
                id: 55,
                phase: ContactPhase::Up,
                kind: PointerKind::Mouse,
                button: if name == "Hand" {
                    PointerButton::Primary
                } else {
                    PointerButton::Pan
                },
                position: [600.0, 450.0],
                time_ns: 0,
            });
        }
        pump(150);
        if name == "Move" {
            assert_ne!(
                ui_session(&w).engine().document().working.selection,
                selection_before,
                "pacing must move the selected pixels: {:?}",
                state(&w).notice
            );
        }
        if let Some(preset) = preset {
            let gpu = w.gpu.borrow();
            let engine = gpu.as_ref().unwrap().session.engine();
            assert_eq!(
                engine.metrics().committed_strokes,
                strokes_before + 1,
                "pacing must draw a complete stroke"
            );
            assert_eq!(
                engine.configured_brush().execution,
                layer_core::default_brush(preset).execution
            );
            assert!(
                engine.metrics().input_events > 100,
                "pacing must deliver real samples"
            );
        }
        let stats = worker_stats.lock().unwrap();
        assert!(
            name == "Refine" || stats.cpu.len() > 100,
            "canvas must schedule independently of GTK painting; worker frames = {}, GPU = {}, presented = {}",
            stats.cpu.len(),
            stats.gpu.len(),
            stats.presented.len()
        );
        let gpu_timestamps = std::env::var("LAYER_PACING_GPU_TIMESTAMPS").as_deref() != Ok("0");
        if gpu_timestamps {
            assert!(
                name == "Refine" || stats.gpu.len() > 100,
                "hardware timestamps must cover GPU rendering"
            );
        } else {
            assert!(
                stats.gpu.is_empty(),
                "no query submissions in timestamp-free profiling"
            );
        }
        assert_eq!(stats.thread_cpu.len(), stats.cpu.len());
        assert!(
            stats
                .thread_cpu
                .iter()
                .flatten()
                .all(|v| v.is_finite() && *v >= 0.)
        );
        assert!(
            name == "Refine" || stats.presented.iter().filter(|p| p[3] == 1).count() > 100,
            "measure real child-surface presentation"
        );
        let report = serde_json::json!({
            "brush": name, "viewport": camera.viewport, "brush_size": brush_size, "stroke_seconds": 6,
            "startup_wait_ms": startup_wait_ms,
            "gpu_timestamps": gpu_timestamps,
            "workspace": std::env::var("LAYER_PACING_WORKSPACE").unwrap_or_else(|_| "default".into()),
            "gtk_renderer": w.window.renderer().unwrap().type_().name(),
            "path": "app-owned Wayland Vulkan subsurface",
            "navigator": navigator_visible,
            "navigator_updates": preview_updates,
            "navigator_frames": stats.overview_frames,
            "transform_mask": std::env::var("LAYER_PACING_TRANSFORM_MASK").unwrap_or_default(),
            "transform_mode": if distort { "distort" } else if outline { "outline" } else { "free" },
            "move_selection": std::env::var("LAYER_PACING_MOVE").unwrap_or_default(),
            "leave_copy": std::env::var("LAYER_PACING_LEAVE_COPY").as_deref() == Ok("1"),
            "interpolation": interpolation,
            "input_cpu": stats.input_cpu,
            "input_handler_cpu": stats.input_handler_cpu,
            "frame_handler_cpu": stats.frame_handler_cpu,
            "gtk_paint_cpu": *paint_cpu.borrow(),
            "main_dispatch_thread_cpu": dispatch_thread_cpu,
            "event_loop": "native_wait",
            "input_events": input_events, "input_hz": input_events as f64 / input_seconds,
            "wake_lateness": stats.wake_lateness,
            "worker_cpu": stats.cpu, "worker_cpu_stages": stats.cpu_stages,
            "worker_thread_cpu": stats.thread_cpu,
            "worker_gpu": stats.gpu, "canvas_presentation": stats.presented,
            "transparency": format!("{:?}", state(&w).settings.transparency),
            "backdrop_frames": stats.backdrop_frames,
        });
        let extent = {
            let gpu = w.gpu.borrow();
            let doc = gpu.as_ref().unwrap().session.engine().document();
            [doc.composition().size[0], doc.composition().size[1]]
        };
        let mut report = report;
        report["document"] = serde_json::json!(extent);
        if name == "Refine" {
            report["refine"] = serde_json::json!({
                "values": refine_values,
                "document_revisions": refine_revisions,
                "previews": refine_preview_count(&w) - refine_previews,
            });
        }
        eprintln!(
            "{name}: {} canvas frames, {} GPU timings, {} presentation feedbacks",
            stats.cpu.len(),
            stats.gpu.len(),
            stats.presented.len()
        );
        reports.push(report);
        drop(stats);
        finish_canvas_operation(&w);
    }
    assert!(
        !reports.is_empty(),
        "LAYER_PACING_BRUSH did not match a benchmark workload"
    );
    let path = std::env::var("LAYER_PACING_REPORT")
        .unwrap_or_else(|_| "/tmp/layer-wayland-pacing.json".into());
    std::fs::write(path, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_window_drag_input() {
    let app = native_test_app("art.capycanvas.WindowDragInput");
    let w = fixture_workspace(&app);
    w.window.set_default_size(1100, 760);
    let observed = Rc::new(Cell::new(None));
    let motion = gtk::EventControllerMotion::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    motion.connect_motion(glib::clone!(
        #[weak]
        w,
        #[strong]
        observed,
        move |controller, x, y| {
            let toolkit = w
                .window
                .compute_point(&w.surface, &gtk::graphene::Point::new(x as f32, y as f32))
                .unwrap();
            observed.set(Some((
                [toolkit.x(), toolkit.y()],
                w.event_point(controller).unwrap(),
            )));
        }
    ));
    w.window.add_controller(motion);
    w.window.present();
    pump(1200);
    wait_workspaces(&w);
    // Storage adopts the shipped preset asynchronously after realization.
    // This geometry test uses the fixed tab IDs, not that startup layout.
    let original = layer_ui::WorkspaceState::default();
    let saved = |w: &Workspace| serde_json::to_value(state(w).workspace).unwrap();
    let mut input = RemoteInput::new().settle_ms(180).timeout_secs(4);
    input.ready();
    for mode in ["windowed", "maximized", "fullscreen", "restored"] {
        match mode {
            "maximized" => w.window.maximize(),
            "fullscreen" => {
                w.window.unmaximize();
                pump(400); // Let the compositor save the restored geometry before fullscreen.
                w.window.fullscreen();
            }
            "restored" => w.window.unfullscreen(),
            _ => (),
        }
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(original.clone()),
        });
        pump(500);
        // Wayland does not expose global window positions to clients. Calibrate
        // the test pointer using GTK's widget-local motion, independently of
        // the production raw-event transform being checked here.
        observed.set(None);
        input.perform(serde_json::json!([{ "point": [799., 499.] }, { "point": [800., 500.] }]));
        let (toolkit, raw) = observed.get().expect("pointer inside the test window");
        let mut origin = [800. - toolkit[0], 500. - toolkit[1]];
        assert!(
            (raw[0] - toolkit[0]).abs() < 0.5 && (raw[1] - toolkit[1]).abs() < 0.5,
            "{mode}: raw workspace point {raw:?} differs from GTK {toolkit:?}, surface transform {:?}",
            w.window.surface_transform()
        );
        if matches!(mode, "windowed" | "restored") {
            assert!(!w.window.is_maximized() && !w.window.is_fullscreen());
            assert_ne!(
                w.window.surface_transform(),
                (0., 0.),
                "exercise CSD shadow offsets"
            );
            let color = state(&w)
                .workspace
                .layout
                .panel(Panel::Toolbar)
                .unwrap()
                .tiles()
                .iter()
                .find(|tile| tile.control == ToolbarControl::Color)
                .unwrap()
                .id;
            w.dispatch(UiAction::ActivateTile {
                panel: Panel::Toolbar,
                tile: color,
            });
            pump(300);
            assert!(state(&w).customization.drawer.is_some());
            let before = saved(&w);
            let title = find_css(w.header.root.upcast_ref(), "document-title").unwrap();
            let b = title.compute_bounds(&w.surface).unwrap();
            let start = [
                origin[0] + b.x() + b.width() * 0.5,
                origin[1] + b.y() + b.height() * 0.5,
            ];
            input.perform(
                serde_json::json!([{ "point": start }, { "down": true }, { "point": [start[0] + 20., start[1] + 10.] }, { "point": [start[0] + 80., start[1] + 40.] }, { "down": false }]),
            );
            input
                .perform(serde_json::json!([{ "point": [799., 499.] }, { "point": [800., 500.] }]));
            let (point, _) = observed.get().unwrap();
            let moved = [800. - point[0], 500. - point[1]];
            assert!(
                (moved[0] - origin[0]).abs() > 30. && (moved[1] - origin[1]).abs() > 10.,
                "title bar must move the {mode} window: {origin:?} -> {moved:?}; start={start:?}, title={b:?}, size={}x{}, active={}, drag={}",
                w.window.width(),
                w.window.height(),
                w.window.is_active(),
                w.workspace_drag.borrow().is_some()
            );
            assert_eq!(saved(&w), before, "window movement must not drag a panel");
            assert!(
                state(&w).customization.drawer.is_none(),
                "The same title-bar contact dismisses the drawer and moves the window"
            );
            assert!(w.workspace_drag.borrow().is_none());
            assert!(!w.chrome_held.get());
            origin = moved;
        }
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let global = |p: [f32; 2]| [p[0] + origin[0], p[1] + origin[1]];
        // Normal docked tabs and divider handles must use the same coordinates.
        let before = saved(&w);
        let tab = w
            .tab_hits()
            .into_iter()
            .find(|t| t.group == 8 && t.index == 0)
            .unwrap()
            .bounds;
        let start = global([tab.x + tab.width * 0.5, tab.y + tab.height * 0.5]);
        let away = global([viewport[0] * 0.5, viewport[1] * 0.55]);
        input.perform(serde_json::json!([{ "point": start }, { "down": true }, { "point": away }]));
        let bottom = global([viewport[0] * 0.5, viewport[1] - 2.]);
        input.perform(serde_json::json!([{ "point": bottom }]));
        let preview = ui_session(&w)
            .workspace_update()
            .drag
            .unwrap()
            .group
            .unwrap();
        let native = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == preview.id)
            .unwrap()
            .root
            .compute_bounds(&w.surface)
            .unwrap();
        assert!(
            preview.bounds.y + preview.bounds.height > viewport[1],
            "{mode}: panel must follow the contact beyond the workspace bottom"
        );
        assert!((native.y() - preview.bounds.y).abs() < 1.);
        input.perform(serde_json::json!([{ "down": false }]));
        let fitted = w
            .resolved()
            .groups
            .into_iter()
            .find(|g| g.id == preview.id)
            .unwrap()
            .bounds;
        assert!(fitted.y + fitted.height <= viewport[1] - WORKSPACE_SPACING + 1.);
        assert_eq!(
            state(&w).workspace.layout.floating.len(),
            1,
            "tab tear-off in {mode}"
        );
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        pump(200);
        assert_eq!(saved(&w), before);
        let divider = w
            .resolved()
            .dividers
            .into_iter()
            .find(|d| d.band && d.id == 7)
            .unwrap();
        let start = global([
            divider.bounds.x + divider.bounds.width * 0.5,
            divider.bounds.y + 150.,
        ]);
        let end = [start[0] - 40., start[1]];
        input.perform(
            serde_json::json!([{ "point": start }, { "down": true }, { "point": end }, { "down": false }]),
        );
        assert_ne!(saved(&w), before, "divider resize in {mode}");
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        pump(200);
        assert_eq!(saved(&w), before);
        w.dispatch(UiAction::DoubleClickPanelHandle { group: 8, viewport });
        enable_individual_column_panels(&w, 8);
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::ToggleColumnDrawer {
                group: 8,
                panel: Panel::Layers,
            },
        });
        pump(400);
        let collapsed = saved(&w);
        let root = find_named(w.surface.upcast_ref(), "column-drawer-8").unwrap();
        let grip = find_named(&root, "column-drawer-grip").unwrap();
        let b = grip.compute_bounds(&w.surface).unwrap();
        let start = global([b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]);
        input.perform(
            serde_json::json!([{ "point": start }, { "down": true }, { "point": away }, { "down": false }]),
        );
        let layout = state(&w).workspace.layout;
        assert_eq!(layout.floating.len(), 1, "drawer group tear-off in {mode}");
        assert_eq!(
            layout
                .group_panels(layout.panel_group(Panel::Layers).unwrap())
                .unwrap()
                .len(),
            3
        );
        let floating = saved(&w);
        let source = w
            .resolved()
            .groups
            .into_iter()
            .find(|g| g.floating)
            .unwrap()
            .bounds;
        let target = w
            .resolved()
            .groups
            .into_iter()
            .find(|g| g.id == 5)
            .unwrap()
            .bounds;
        let start = global([source.x + source.width - 10., source.y + 18.]);
        let end = global([target.x + target.width * 0.5, target.y + 18.]);
        input.perform(
            serde_json::json!([{ "point": start }, { "down": true }, { "point": end }, { "down": false }]),
        );
        assert_eq!(
            state(&w).workspace.layout.panel_group(Panel::Layers),
            Some(5),
            "group drop in {mode}"
        );
        assert!(state(&w).workspace.layout.floating.is_empty());
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        pump(200);
        assert_eq!(saved(&w), floating);
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        pump(200);
        assert_eq!(saved(&w), collapsed);
    }
    input.finish();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_long_press_drag_input() {
    let app = native_test_app("art.capycanvas.LongPressDragInput");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    let original = state(&w).workspace;
    let saved = || serde_json::to_value(state(&w).workspace).unwrap();
    let mut input = RemoteInput::new().settle_ms(150).timeout_secs(4);
    input.ready();
    let menu = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .find(|p| p.has_css_class("panel-context-menu"))
        .unwrap();
    for mode in [
        "release",
        "reorder",
        "detach",
        "drawer",
        "grip",
        "cancel",
        "floating-tab",
        "floating-group",
        "toolbar-grip",
        "tool",
        "tool-cancel",
    ] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(original.clone()),
        });
        pump(300);
        let group = original.layout.panel_group(Panel::Adjustments).unwrap();
        if mode == "drawer" {
            w.dispatch(UiAction::DoubleClickPanelHandle {
                group,
                viewport: [w.surface.width() as f32, w.surface.height() as f32],
            });
            enable_individual_column_panels(&w, group);
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::ToggleColumnDrawer {
                    group,
                    panel: Panel::Layers,
                },
            });
            pump(300);
        }
        if mode.starts_with("floating") {
            w.dispatch(UiAction::MoveGroup {
                group,
                target: DockTarget::Float {
                    position: [600., 250.],
                },
                viewport: [w.surface.width() as f32, w.surface.height() as f32],
            });
            pump(250);
        }
        let tile = mode == "tool" || mode == "tool-cancel";
        let hits = w.tab_hits();
        let index = original
            .layout
            .group_panels(group)
            .unwrap()
            .iter()
            .position(|p| *p == Panel::Adjustments)
            .unwrap();
        let tab = hits
            .iter()
            .find(|t| t.group == group && t.index == index)
            .unwrap()
            .bounds;
        let mut start = [tab.x + tab.width * 0.5, tab.y + tab.height * 0.5];
        if mode == "grip" || mode == "floating-group" {
            let group_bounds = w
                .resolved()
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds;
            start = [group_bounds.x + group_bounds.width - 10., start[1]];
        }
        if tile || mode == "toolbar-grip" {
            let widget = if tile {
                w.toolbar.first_child().unwrap()
            } else {
                find_css(w.toolbar.upcast_ref(), "panel-grip").unwrap()
            };
            let bounds = widget.compute_bounds(&w.surface).unwrap();
            start = [
                bounds.x() + bounds.width() * 0.5,
                bounds.y() + bounds.height() * 0.5,
            ];
        }
        let before = saved();
        input.perform(serde_json::json!([{"touch":"down", "point":start}]));
        pump(900);
        assert!(
            menu.is_visible(),
            "{mode}: real touch hold opens the context menu; pending {}",
            w.workspace_drag.borrow().is_some()
        );
        assert_eq!(
            saved(),
            before,
            "{mode}: holding does not select or move a tab"
        );
        input.perform(serde_json::json!([{"touch":"move", "point":[start[0] + 2., start[1]]}]));
        assert!(menu.is_visible(), "small movement keeps the menu open");
        if mode == "release" {
            input.perform(serde_json::json!([{"touch":"up"}]));
            assert!(menu.is_visible(), "hold release leaves the menu available");
            assert_eq!(saved(), before, "hold release does not select a tab");
            assert!(menu.is_autohide());
            input.perform(
                serde_json::json!([{"touch":"down", "point":[800., 500.]}, {"touch":"up"}]),
            );
            assert!(
                !menu.is_visible(),
                "a subsequent outside tap dismisses the menu"
            );
            assert!(w.workspace_drag.borrow().is_none());
            input.perform(serde_json::json!([{"touch":"down", "point":start}, {"touch":"up"}]));
            assert_eq!(
                state(&w).workspace.layout.panel_group(Panel::Adjustments),
                Some(group)
            );
            assert_eq!(
                w.resolved()
                    .groups
                    .iter()
                    .find(|g| g.id == group)
                    .unwrap()
                    .active,
                Panel::Adjustments,
                "the first ordinary tap after a hold still selects its tab"
            );
            continue;
        }
        let first = hits
            .iter()
            .find(|t| t.group == group && t.index == 0)
            .unwrap()
            .bounds;
        let point = if tile {
            let widget = w
                .toolbar
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .next_sibling()
                .unwrap();
            let b = widget.compute_bounds(&w.surface).unwrap();
            [b.x() + b.width() * 0.8, b.y() + b.height() * 0.8]
        } else if ["reorder", "drawer"].contains(&mode) {
            [first.x + 2., start[1]]
        } else {
            [
                w.surface.width() as f32 * 0.5,
                w.surface.height() as f32 * 0.55,
            ]
        };
        input.perform(serde_json::json!([{"touch":"move", "point":point}]));
        assert!(
            !menu.is_visible(),
            "{mode}: movement dismisses the context menu"
        );
        assert!(
            w.workspace_drag
                .borrow()
                .as_ref()
                .is_some_and(|d| d.started),
            "{mode}: same touch starts dragging"
        );
        if mode == "cancel" || mode == "tool-cancel" {
            w.interact(UiInput::Blur);
        }
        input.perform(serde_json::json!([{"touch":"up"}]));
        assert!(w.workspace_drag.borrow().is_none());
        if mode != "cancel" && mode != "tool-cancel" {
            let layout = state(&w).workspace.layout;
            if tile || mode.starts_with("floating") || mode == "toolbar-grip" {
                assert_ne!(saved(), before, "{mode}: drop moves the element");
            } else if ["reorder", "drawer"].contains(&mode) {
                assert_eq!(layout.group_panels(group).unwrap()[0], Panel::Adjustments);
            } else {
                assert_eq!(layout.floating.len(), 1);
                let panels = layout
                    .group_panels(layout.panel_group(Panel::Adjustments).unwrap())
                    .unwrap();
                assert_eq!(
                    panels.len(),
                    if mode == "grip" || mode == "floating-group" {
                        original.layout.group_panels(group).unwrap().len()
                    } else {
                        1
                    }
                );
            }
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            pump(200);
        }
        assert_eq!(saved(), before, "the continued drag is one undo step");
    }
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(original.clone()),
    });
    for _ in 0..2 {
        w.dispatch(UiAction::Layer {
            action: layer_ui::LayerAction::New {
                group: false,
                clipped: false,
            },
        });
    }
    pump(300);
    let order = || state(&w).layers.iter().map(|l| l.id).collect::<Vec<_>>();
    let before = order();
    let source = find_named(
        w.layer_panel.root.upcast_ref(),
        &format!("art-layer-{}", before[0]),
    )
    .unwrap();
    let grip = source.last_child().unwrap();
    let b = grip.compute_bounds(&w.surface).unwrap();
    let start = [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5];
    input.perform(serde_json::json!([{"touch":"down", "point":start}]));
    pump(900);
    let layer_menu = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .find(|p| p.is_visible())
        .unwrap();
    let target = find_named(
        w.layer_panel.root.upcast_ref(),
        &format!("art-layer-{}", before[1]),
    )
    .unwrap();
    let b = target.compute_bounds(&w.surface).unwrap();
    let point = [b.x() + b.width() * 0.5, b.y() + b.height() - 3.];
    input.perform(serde_json::json!([{"touch":"move", "point":point}]));
    assert!(
        !layer_menu.is_visible(),
        "layer grip movement dismisses its context menu"
    );
    input.perform(
        serde_json::json!([{"touch":"move", "point":[point[0] + 1., point[1]]}, {"touch":"up"}]),
    );
    assert_ne!(order(), before, "the same held layer grip drops the layer");
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(200);
    assert_eq!(order(), before);
    input.finish();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_tab_slide_input() {
    let app = native_test_app("art.capycanvas.TabSlideInput");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    let original = layer_ui::WorkspaceState::default();
    let mut input = RemoteInput::new().settle_ms(150).timeout_secs(4);
    input.ready();
    for (panel, end) in [
        (Panel::Layers, "cancel"),
        (Panel::Adjustments, "release"),
        (Panel::Layers, "blur"),
        (Panel::Adjustments, "detach"),
    ] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(original.clone()),
        });
        pump(300);
        let group = original.layout.panel_group(panel).unwrap();
        let tab = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .tabs
            .iter()
            .find(|(p, _)| *p == panel)
            .unwrap()
            .1
            .clone();
        let bounds = tab.compute_bounds(&w.surface).unwrap();
        let start = [
            bounds.x() + bounds.width() * 0.5,
            bounds.y() + bounds.height() * 0.5,
        ];
        let before_hits = w.tab_hits();
        input.perform(serde_json::json!([{ "point": start }, { "down": true },
            { "point": [start[0] + 2., start[1]] }]));
        assert!(w.workspace_drag.borrow().as_ref().unwrap().tab.is_none());
        assert_eq!(tab.opacity(), 1.);
        for dx in [24., -16.] {
            input.perform(serde_json::json!([{ "point": [start[0] + dx, start[1] + 8.] }]));
            assert_eq!(tab.opacity(), 0.);
            assert_eq!(tab.compute_bounds(&w.surface).unwrap(), bounds);
            assert_eq!(
                w.tab_hits(),
                before_hits,
                "Visual sliding must not move insertion targets"
            );
            assert_eq!(
                serde_json::to_value(state(&w).workspace).unwrap(),
                serde_json::to_value(&original).unwrap()
            );
            let drag = w.workspace_drag.borrow();
            let drag = drag.as_ref().unwrap();
            let slide = drag.tab.as_ref().unwrap();
            assert!(
                (slide.bounds.x
                    - (bounds.x() + dx).clamp(
                        slide.clip.x,
                        (slide.clip.x + slide.clip.width - bounds.width()).max(slide.clip.x)
                    ))
                .abs()
                    < 1.
            );
            assert_eq!(slide.bounds.y, bounds.y());
            assert!((drag.point[0] - drag.origin[0] - dx).abs() < 1.);
        }
        let clip = w
            .workspace_drag
            .borrow()
            .as_ref()
            .unwrap()
            .tab
            .as_ref()
            .unwrap()
            .clip;
        for x in [clip.x - 40., clip.x + clip.width + 10.] {
            input.perform(serde_json::json!([{ "point": [x, start[1]] }]));
            let drag = w.workspace_drag.borrow();
            let slide = drag.as_ref().unwrap().tab.as_ref().unwrap();
            assert!(
                (slide.bounds.x
                    - if x < clip.x {
                        clip.x
                    } else {
                        clip.x + clip.width - bounds.width()
                    })
                .abs()
                    < 1.
            );
            assert!(state(&w).workspace.layout.floating.is_empty());
        }
        let first = before_hits
            .iter()
            .find(|hit| hit.group == group && hit.index == 0)
            .unwrap()
            .bounds;
        let neighbor = before_hits
            .iter()
            .find(|hit| hit.group == group && hit.index == 1)
            .unwrap()
            .bounds;
        // Cross only halfway over the adjacent tab, pause, and reverse across
        // that same boundary. The pointer has not reached the old midpoint.
        let threshold = if panel == Panel::Layers {
            neighbor.width * 0.5
        } else {
            -first.width * 0.5
        };
        let direction = threshold.signum();
        let target_x = start[0] + threshold + direction * 2.;
        for crossed in [false, true, true, false] {
            let x = start[0] + threshold + direction * if crossed { 2. } else { -2. };
            input.perform(serde_json::json!([{ "point": [x, start[1]] }]));
            let drag = w.workspace_drag.borrow();
            let slide = drag.as_ref().unwrap().tab.as_ref().unwrap();
            assert_eq!(slide.tabs.iter().any(|tab| tab.to != 0.), crossed);
        }
        for _ in 0..2 {
            input.perform(serde_json::json!([{ "point": [target_x, start[1]] }]));
            let drag = w.workspace_drag.borrow();
            let slide = drag.as_ref().unwrap().tab.as_ref().unwrap();
            assert!(
                slide
                    .tabs
                    .iter()
                    .any(|tab| (tab.to.abs() - bounds.width()).abs() < 1.)
            );
            assert_eq!(w.tab_hits(), before_hits);
        }
        crate::snapshot(&w)
            .save_to_png(input.dir.join(format!("tab-slide-{end}.png")))
            .unwrap();
        input.perform(serde_json::json!([{ "point": start }]));
        assert!(
            w.workspace_drag
                .borrow()
                .as_ref()
                .unwrap()
                .tab
                .as_ref()
                .unwrap()
                .tabs
                .iter()
                .all(|tab| tab.to == 0.)
        );
        let mut point = [start[0] - 16., start[1] + 8.];
        if end == "detach" {
            point = [
                w.surface.width() as f32 * 0.5,
                w.surface.height() as f32 * 0.55,
            ];
            input.perform(serde_json::json!([{ "point": point }]));
            assert_eq!(state(&w).workspace.layout.floating.len(), 1);
            let layout = ui_session(&w)
                .layout([w.surface.width() as f32, w.surface.height() as f32]);
            let floated = layout
                .groups
                .iter()
                .find(|g| g.floating && g.active == panel)
                .unwrap();
            assert!(
                (floated.bounds.x - (point[0] - (start[0] - bounds.x()))).abs() < 1.,
                "The pointer stays over the grabbed point within the detached tab"
            );
            assert!(w.workspace_drag.borrow().as_ref().unwrap().tab.is_none());
            assert_eq!(tab.opacity(), 1.);
            w.workspace_drag_input(ContactPhase::Cancel, point, None);
        } else if end == "release" {
            point = [target_x, start[1]];
            input.perform(serde_json::json!([{ "point": point }]));
        } else if end == "blur" {
            w.interact(UiInput::Blur);
        } else {
            w.workspace_drag_input(ContactPhase::Cancel, point, None);
        }
        input.perform(serde_json::json!([{ "down": false }]));
        assert!(w.workspace_drag.borrow().is_none());
        assert_eq!(tab.opacity(), 1.);
        if end == "release" {
            assert_eq!(
                state(&w).workspace.layout.group_panels(group).unwrap()[0],
                panel
            );
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
        }
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
    }
    input.finish();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_column_drawer_drag_input() {
    let app = native_test_app("art.capycanvas.ColumnDrawerDragInput");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    // The asynchronously loaded shipped preset can replace the realized fixture.
    // This regression addresses the fixed fixture's group and tab IDs.
    let original = layer_ui::WorkspaceState::default();
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(original.clone()),
    });
    pump(250);
    let saved = |w: &Workspace| serde_json::to_value(state(w).workspace).unwrap();
    let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
    let mut input = RemoteInput::new().settle_ms(150).timeout_secs(4);
    input.ready();
    let mut perform = |events: serde_json::Value| {
        input.perform(events);
        let native = w.window.surface().unwrap();
        let pointer = native.display().default_seat().unwrap().pointer().unwrap();
        let cursor = native.device_cursor(&pointer).and_then(|c| c.name());
        if w.workspace_drag
            .borrow()
            .as_ref()
            .is_some_and(|d| d.started)
        {
            assert_eq!(cursor.as_deref(), Some("grabbing"));
        } else {
            assert_ne!(cursor.as_deref(), Some("grabbing"));
        }
    };
    for (group, panel, whole) in [
        (8, Panel::Layers, false),
        (8, Panel::Adjustments, false),
        (8, Panel::Layers, true),
        (5, Panel::Brushes, false),
    ] {
        for dock in [false, true] {
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(original.clone()),
            });
            w.dispatch(UiAction::DoubleClickPanelHandle { group, viewport });
            let origin = if group == 8 {
                Panel::Layers
            } else {
                Panel::Brushes
            };
            enable_individual_column_panels(&w, group);
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::ToggleColumnDrawer {
                    group,
                    panel: origin,
                },
            });
            pump(400);
            let before = saved(&w);
            let column = original.layout.column_for_group(group).unwrap();
            let root =
                find_named(w.surface.upcast_ref(), &format!("column-drawer-{column}")).unwrap();
            let grip = find_named(&root, "column-drawer-grip").unwrap();
            let gb = grip.compute_bounds(&w.surface).unwrap();
            let bounds = w
                .columns
                .drawers
                .borrow()
                .iter()
                .find(|d| d.id == column)
                .unwrap()
                .placement()
                .unwrap()
                .bounds;
            if (gb.x() + gb.width() - bounds.x - bounds.width).abs() >= 2.
                && let Some(directory) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
                capture_reference(&w, std::path::PathBuf::from(directory).join("drawer-grip-mismatch.png").to_str().unwrap(), 1.);
            }
            assert!(
                (gb.x() + gb.width() - bounds.x - bounds.width).abs() < 2.,
                "fixed top-right grip: native {gb:?}, resolved {bounds:?}, group {group}, panel {panel:?}"
            );
            let start = if whole {
                [gb.x() + gb.width() * 0.5, gb.y() + gb.height() * 0.5]
            } else {
                let index = original
                    .layout
                    .group_panels(group)
                    .unwrap()
                    .iter()
                    .position(|p| *p == panel)
                    .unwrap();
                center(
                    w.tab_hits()
                        .into_iter()
                        .find(|t| t.group == group && t.index == index)
                        .unwrap()
                        .bounds,
                )
            };
            let item = if whole {
                DockItem::Group { group }
            } else {
                DockItem::Panel { panel }
            };
            assert!(
                matches!(w.drag_target_at(start), Some(DragTarget::Dock(target)) if target == item)
            );
            let away = [[12., 12.], [viewport[0] - 12., 12.], [12., viewport[1] - 12.], [viewport[0] - 12., viewport[1] - 12.]]
                .into_iter().max_by(|a, b| bounds.distance_to(*a).total_cmp(&bounds.distance_to(*b))).unwrap();
            assert!(!bounds.contains(away[0], away[1]));
            perform(
                serde_json::json!([{ "point": start }, { "down": true }, { "point": [start[0] - 12., start[1]] }]),
            );
            assert!(
                w.workspace_drag
                    .borrow()
                    .as_ref()
                    .is_some_and(|d| d.started)
            );
            assert_eq!(
                saved(&w),
                before,
                "inside the drawer retains the source dock"
            );
            assert_eq!(
                w.workspace_drag.borrow().as_ref().unwrap().tab.is_some(),
                !whole
            );
            if !whole {
                let drag = w.workspace_drag.borrow();
                let drag = drag.as_ref().unwrap();
                assert!(
                    drag.tab
                        .as_ref()
                        .unwrap()
                        .tabs
                        .iter()
                        .all(|tab| tab.widget.opacity() == 0.)
                );
                assert!((drag.point[0] - drag.origin[0] + 12.).abs() < 1.);
            }
            perform(serde_json::json!([{ "point": away }]));
            if w.workspace_drag.borrow().as_ref().unwrap().tab.is_some() {
                let drag = w.workspace_drag.borrow();
                let drag = drag.as_ref().unwrap();
                eprintln!("Drawer detach {group} {panel:?} whole={whole} bounds={bounds:?} start={start:?} away={away:?} actual={:?} origin={:?}", drag.point, drag.origin);
                if let Some(path) = std::env::var_os("LAYER_TEST_ARTIFACTS") { capture_reference(&w, std::path::PathBuf::from(path).join(format!("drawer-detach-{group}-{panel:?}-{whole}.png")).to_str().unwrap(), 1.); }
            }
            assert!(w.workspace_drag.borrow().as_ref().unwrap().tab.is_none());
            assert_eq!(
                state(&w).workspace.layout.floating.len(),
                1,
                "{group} {panel:?} {whole}"
            );
            if !dock {
                // Cancellation restores both the dock and the open drawer.
                assert!(w.workspace_drag_input(ContactPhase::Cancel, away, None));
                perform(serde_json::json!([{ "down": false }]));
                assert_eq!(saved(&w), before);
                assert_eq!(state(&w).customization.column_drawers.len(), 1);
                perform(
                    serde_json::json!([{ "point": start }, { "down": true }, { "point": away }]),
                );
            }
            let destination = if dock {
                let target = w
                    .resolved()
                    .groups
                    .into_iter()
                    .find(|g| g.id == if group == 5 { 8 } else { 5 })
                    .unwrap();
                [
                    target.bounds.x + target.bounds.width * 0.5,
                    target.bounds.y + 18.,
                ]
            } else {
                let point = center(w.resolved().work_area);
                assert!(w.drop_at(point[0], point[1], item).is_none(), "canvas release has no docking target");
                point
            };
            perform(serde_json::json!([{ "point": destination }, { "down": false }]));
            assert!(w.workspace_drag.borrow().is_none());
            let layout = state(&w).workspace.layout;
            assert_eq!(layout.floating.len(), usize::from(!dock));
            let destination_group = layout.panel_group(panel).unwrap();
            if dock {
                assert_eq!(destination_group, if group == 5 { 8 } else { 5 });
            }
            if whole {
                assert_eq!(
                    layout.group_panels(destination_group).unwrap().len(),
                    if dock { 4 } else { 3 }
                );
            } else if group == 8 {
                assert_eq!(layout.group_panels(group).unwrap().len(), 2);
                assert!(layout.is_collapsed(column));
            }
            layout.validate().unwrap();
            let after = saved(&w);
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            pump(200);
            assert_eq!(saved(&w), before);
            w.dispatch(UiAction::Invoke {
                command: CommandId::RedoWorkspace,
            });
            pump(200);
            assert_eq!(saved(&w), after);
        }
    }
    // Drawer tabs and the upper body insert precisely; middle drops prepend.
    for (target_group, whole, zone) in [(8, false, 0), (8, true, 1), (5, true, 0), (8, false, 2)] {
        let mut workspace = original.clone();
        if target_group == 8 && whole {
            workspace
                .layout
                .move_panel(
                    viewport,
                    Panel::Sizes,
                    DockTarget::Tab {
                        group: 5,
                        index: None,
                    },
                )
                .unwrap();
        }
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(workspace),
        });
        w.dispatch(UiAction::DoubleClickPanelHandle {
            group: target_group,
            viewport,
        });
        let origin = if target_group == 8 {
            Panel::Layers
        } else {
            Panel::Brushes
        };
        enable_individual_column_panels(&w, target_group);
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::ToggleColumnDrawer {
                group: target_group,
                panel: origin,
            },
        });
        pump(400);
        let before = saved(&w);
        let source_group = if target_group == 8 { 5 } else { 8 };
        let panel = if target_group == 8 {
            Panel::Brushes
        } else {
            Panel::Layers
        };
        let source = w
            .resolved()
            .groups
            .into_iter()
            .find(|g| g.id == source_group)
            .unwrap();
        let start = if whole {
            [
                source.bounds.x + source.bounds.width - 10.,
                source.bounds.y + 18.,
            ]
        } else {
            center(
                w.tab_hits()
                    .into_iter()
                    .find(|t| t.group == source_group && t.index == 0)
                    .unwrap()
                    .bounds,
            )
        };
        let item = if whole {
            DockItem::Group {
                group: source_group,
            }
        } else {
            DockItem::Panel { panel }
        };
        assert!(
            matches!(w.drag_target_at(start), Some(DragTarget::Dock(target)) if target == item)
        );
        let bounds = w.columns.drawers.borrow()[0].placement().unwrap().bounds;
        let destination = match zone {
            0 => [bounds.x + 4., bounds.y + 18.],
            1 => center(bounds),
            _ => [
                bounds.x + 4.,
                bounds.y + TAB_BAR_HEIGHT + 4.,
            ],
        };
        perform(
            serde_json::json!([{ "point": start }, { "down": true }, { "point": [viewport[0] * 0.5, viewport[1] * 0.6] }, { "point": destination }]),
        );
        let hint = w
            .drop_hint
            .borrow()
            .clone()
            .unwrap_or_else(|| panic!("open drawer drop indicator: target={target_group}, whole={whole}, zone={zone}, point={destination:?}, start={start:?}, drag={}, drawers={:?}, floating={}, direct={:?}", w.workspace_drag.borrow().is_some(), state(&w).customization.column_drawers, state(&w).workspace.layout.floating.len(), w.drop_at(destination[0], destination[1], item)));
        let expected = DockTarget::Tab {
            group: target_group,
            index: Some(0),
        };
        assert_eq!(hint.target, expected);
        if zone == 2 {
            assert_eq!(hint.bounds.y, bounds.y);
            assert_eq!(hint.bounds.width, 3.);
        }
        perform(serde_json::json!([{ "down": false }]));
        let layout = state(&w).workspace.layout;
        let destination_group = layout.panel_group(panel).unwrap();
        assert_eq!(destination_group, target_group);
        assert_eq!(layout.group_panels(target_group).unwrap()[0], panel);
        assert!(
            layout
                .collapsed_column_for_group(destination_group)
                .is_some()
        );
        assert!(layout.floating.is_empty());
        assert_eq!(state(&w).customization.column_drawers.len(), 1);
        layout.validate().unwrap();
        let after = saved(&w);
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        pump(200);
        assert_eq!(saved(&w), before);
        w.dispatch(UiAction::Invoke {
            command: CommandId::RedoWorkspace,
        });
        pump(200);
        assert_eq!(saved(&w), after);
    }
    // A real header drag can also reorder tabs without leaving the drawer.
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(original),
    });
    w.dispatch(UiAction::DoubleClickPanelHandle { group: 8, viewport });
    enable_individual_column_panels(&w, 8);
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::ToggleColumnDrawer {
            group: 8,
            panel: Panel::Layers,
        },
    });
    pump(400);
    let start = center(
        w.tab_hits()
            .into_iter()
            .find(|t| t.group == 8 && t.index == 0)
            .unwrap()
            .bounds,
    );
    let bounds = w.columns.drawers.borrow()[0].placement().unwrap().bounds;
    let end = [bounds.x + bounds.width - 10., start[1]];
    perform(
        serde_json::json!([{ "point": start }, { "down": true }, { "point": end }, { "down": false }]),
    );
    let layout = state(&w).workspace.layout;
    assert_eq!(
        layout.group_panels(8).unwrap(),
        &[Panel::Adjustments, Panel::Properties, Panel::Layers]
    );
    assert!(layout.is_collapsed(8));
    assert!(layout.floating.is_empty());
    input.finish();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_divider_cursor_input() {
    let app = native_test_app("art.capycanvas.DividerCursorInput");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let original = layer_ui::WorkspaceState::default();
    let native = w.window.surface().unwrap();
    let pointer = native.display().default_seat().unwrap().pointer().unwrap();
    let cursor = || native.device_cursor(&pointer).and_then(|c| c.name());
    let mut input = RemoteInput::new().settle_ms(150).timeout_secs(4);
    input.ready();
    for group in [5, 8] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(original.clone()),
        });
        w.dispatch(UiAction::DoubleClickPanelHandle { group, viewport });
        pump(150);
        let root = original.layout.column_for_group(group).unwrap();
        let band = original
            .layout
            .bands
            .iter()
            .find(|b| b.root.id() == root)
            .unwrap()
            .id;
        let divider = w
            .resolved()
            .dividers
            .into_iter()
            .find(|d| d.id == band)
            .unwrap();
        let handle = w
            .surface
            .imp()
            .children
            .borrow()
            .iter()
            .find(|(slot, _)| *slot == Slot::Divider(band))
            .unwrap()
            .1
            .clone();
        let start = [
            divider.bounds.x + divider.bounds.width * 0.5,
            divider.bounds.y + 80.,
        ];
        let outward = if divider.reversed { -1. } else { 1. };
        input.perform(serde_json::json!([{ "point": start }]));
        assert_eq!(cursor().as_deref(), Some("col-resize"));
        input.perform(
            serde_json::json!([{ "down": true }, { "point": [start[0] + outward * 18., start[1]] }]),
        );
        assert_eq!(cursor().as_deref(), Some("col-resize"));
        for (distance, collapsed) in [
            (36., false),
            (40., false),
            (35., true),
            (36., false),
            (35., true),
            (40., false),
            (36., false),
        ] {
            input.perform(
                serde_json::json!([{ "point": [start[0] + outward * distance, start[1]] }]),
            );
            assert_eq!(state(&w).workspace.layout.is_collapsed(root), collapsed);
            assert_eq!(
                cursor().as_deref(),
                Some("col-resize"),
                "resize cursor while the pointer is inside the expanded panel"
            );
            assert_eq!(handle.parent().as_ref(), Some(w.surface.upcast_ref()));
            assert!(
                w.surface
                    .imp()
                    .children
                    .borrow()
                    .iter()
                    .any(|(slot, widget)| *slot == Slot::Divider(band) && *widget == handle)
            );
        }
        let minimum = if group == 5 {
            layer_ui::TOOL_PANEL_MIN_WIDTH
        } else {
            layer_ui::LAYERS_MIN_WIDTH
        };
        let threshold = minimum - TILE_SIZE - TILE_SIZE;
        for (distance, collapsed) in [
            (minimum - TILE_SIZE + 20., false),
            (threshold - 2., true),
            (threshold + 2., false),
            (threshold - 2., true),
            (threshold + 2., false),
        ] {
            input.perform(
                serde_json::json!([{ "point": [start[0] + outward * distance, start[1]] }]),
            );
            assert_eq!(state(&w).workspace.layout.is_collapsed(root), collapsed);
            assert_eq!(cursor().as_deref(), Some("col-resize"));
        }
        input.perform(serde_json::json!([
            { "down": false },
            { "point": [start[0] + outward * (threshold + 3.), start[1]] }
        ]));
        assert!(w.workspace_drag.borrow().is_none());
        assert_ne!(
            cursor().as_deref(),
            Some("col-resize"),
            "release restores the hovered widget's cursor"
        );

        // Exercise real GTK double-click recognition after the held resize.
        let before = state(&w).workspace;
        let d = w
            .resolved()
            .dividers
            .into_iter()
            .find(|d| d.id == band)
            .unwrap();
        let point = [d.bounds.x + d.bounds.width * 0.5, d.bounds.y + 80.];
        input.perform(serde_json::json!([
            { "point": point }, { "down": true }, { "down": false },
            { "down": true }, { "down": false }
        ]));
        let after = state(&w).workspace;
        let expected = if group == 5 { 242. } else { 254. };
        assert_eq!(
            after
                .layout
                .bands
                .iter()
                .find(|b| b.id == band)
                .unwrap()
                .extent,
            expected + WORKSPACE_SPACING
        );
        assert!(w.workspace_drag.borrow().is_none());
        input.perform(serde_json::json!([{ "point": [point[0] + outward * 20., point[1]] }]));
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(&after).unwrap()
        );
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        w.dispatch(UiAction::Invoke {
            command: CommandId::RedoWorkspace,
        });
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(&after).unwrap()
        );
    }
    // Title bars must collapse both singleton and multi-tab columns. Actual
    // tab buttons keep selection behavior, including on a double-click.
    for group in [8, 5] {
        for grip in [true, false] {
            let mut workspace = original.clone();
            for band in &mut workspace.layout.bands {
                if band.root.id() != 2 {
                    band.extent = 400.;
                }
            }
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(workspace),
            });
            pump(150);
            let root = state(&w).workspace.layout.column_for_group(group).unwrap();
            let (header, tab) = {
                let groups = w.groups.borrow();
                let view = groups.iter().find(|g| g.id == group).unwrap();
                (
                    find_css(view.root.upcast_ref(), "dock-tabs").unwrap(),
                    view.tabs[0].1.clone(),
                )
            };
            let b = tab.compute_bounds(&w.surface).unwrap();
            let point = [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5];
            input.perform(serde_json::json!([
                { "point": point }, { "down": true }, { "down": false },
                { "down": true }, { "down": false }
            ]));
            assert!(
                !state(&w).workspace.layout.is_collapsed(root),
                "tab double-click must not collapse"
            );
            let b = header.compute_bounds(&w.surface).unwrap();
            let point = [
                b.x() + b.width() - if grip { 10. } else { 28. },
                b.y() + b.height() * 0.5,
            ];
            assert!(
                matches!(w.drag_target_at(point), Some(DragTarget::Dock(DockItem::Group { group: id })) if id == group)
            );
            let before = serde_json::to_value(state(&w).workspace).unwrap();
            input.perform(serde_json::json!([
                { "point": point }, { "down": true }, { "down": false },
                { "down": true }, { "down": false }
            ]));
            assert!(
                state(&w).workspace.layout.is_collapsed(root),
                "group {group}, grip {grip}: double-click must collapse"
            );
            assert!(w.workspace_drag.borrow().is_none());
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            assert_eq!(serde_json::to_value(state(&w).workspace).unwrap(), before);
            // Separate clicks outside GTK's double-click interval stay clicks.
            let interval = gtk::Settings::for_display(&w.surface.display())
                .gtk_double_click_time()
                .max(0) as u64;
            for _ in 0..2 {
                pump(interval + 20);
                input.perform(serde_json::json!([
                    { "point": point }, { "down": true }, { "down": false }
                ]));
                assert!(!state(&w).workspace.layout.is_collapsed(root));
            }
            // Recognizing a header click must not prevent a subsequent drag.
            pump(interval + 20);
            input.perform(serde_json::json!([
                { "point": point }, { "down": true },
                { "point": [800., 550.] }, { "down": false }
            ]));
            assert!(
                state(&w)
                    .workspace
                    .layout
                    .floating
                    .iter()
                    .any(|f| f.root.id() == group)
            );
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            assert_eq!(serde_json::to_value(state(&w).workspace).unwrap(), before);
        }
    }
    input.finish();
    pump(100);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_floating_click_input() {
    let app = native_test_app("dev.layer.FloatingClickInputTest");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let initial = state(&w).workspace;
    let mut input = RemoteInput::new().settle_ms(250).timeout_secs(4);
    input.ready();
    for panel in [Panel::Sizes, Panel::Toolbar] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(initial.clone()),
        });
        w.dispatch(UiAction::MovePanel {
            panel,
            viewport,
            target: DockTarget::Float {
                position: [600.0, 250.0],
            },
        });
        pump(250);
        let placement = || {
            w.resolved()
                .groups
                .into_iter()
                .find(|g| g.panels.contains(&panel))
                .unwrap()
        };
        let natural_placement = placement();
        let natural = natural_placement.bounds;
        let natural_tabs = natural_placement.tabs_visible;
        let corner = [
            natural.x + natural.width + 2.0,
            natural.y + natural.height + 2.0,
        ];
        input.perform(serde_json::json!([
            {"point": corner}, {"down": true},
            {"point": [corner[0] + 60.0, corner[1] + 40.0]},
            {"point": [corner[0] + 120.0, corner[1] + 80.0]}, {"down": false}
        ]));
        assert_ne!(placement().bounds, natural, "native resize");
        for cycle in 0..4 {
            let g = placement();
            let grip = g.tiles.as_ref().and_then(|t| t.grip).or(g.footer_grip);
            let point = grip.map_or(
                [g.bounds.x + g.bounds.width - 28.0, g.bounds.y + 12.0],
                |b| [g.bounds.x + b.x + 3.0, g.bounds.y + b.y + 3.0],
            );
            input.perform(serde_json::json!([
                {"point": point}, {"down": true}, {"down": false}, {"down": true}, {"down": false}
            ]));
            let actual = placement();
            capture_reference(
                &w,
                &input
                    .dir
                    .join(format!("{panel:?}-{cycle}.png"))
                    .to_string_lossy(),
                1.0,
            );
            if cycle == 0 {
                assert_eq!(
                    actual.bounds, natural,
                    "first double-click must reset {panel:?}"
                );
            } else if panel == Panel::Toolbar {
                assert_eq!(
                    state(&w).workspace.layout.floating[0].toolbar_layout,
                    [
                        FloatingToolbarLayout::Vertical,
                        FloatingToolbarLayout::Horizontal,
                        FloatingToolbarLayout::Compact
                    ][cycle - 1]
                );
            } else {
                assert_eq!(
                    actual.tabs_visible,
                    natural_tabs ^ (cycle % 2 == 1),
                    "panel toggle {cycle}"
                );
            }
        }
    }
    input.finish();
    pump(100);
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_collapsed_column_input() {
    let app = native_test_app("art.capycanvas.CollapsedColumnInput");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    // A fresh workspace store can finish loading its preset after realization.
    // This test addresses the fixed fixture's group IDs and tab configuration.
    let initial = layer_ui::WorkspaceState::default();
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(initial.clone()),
    });
    pump(250);
    let native = w.window.surface().unwrap();
    let pointer = native.display().default_seat().unwrap().pointer().unwrap();
    let cursor = || native.device_cursor(&pointer).and_then(|c| c.name());
    let mut input = RemoteInput::new().settle_ms(250).timeout_secs(4);
    input.ready();
    let mut double_click = |point: [f32; 2]| {
        input.perform(serde_json::json!([
            {"point": point}, {"down": true}, {"down": false},
            {"down": true}, {"down": false}
        ]));
    };
    for group in [5, 8] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(initial.clone()),
        });
        w.dispatch(UiAction::DoubleClickPanelHandle { group, viewport });
        pump(250);
        let column = w.resolved().collapsed.into_iter().next().unwrap();
        let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
        assert!(find_named(w.surface.upcast_ref(), &format!("expand-column-{}", column.id)).is_none());
        for icon in column.groups.iter().flat_map(|g| &g.icons) {
            let point = center(icon.bounds);
            assert_eq!(w.columns.background_at(&w, point), None);
            double_click(point);
            assert!(state(&w).workspace.layout.is_collapsed(column.id));
            assert!(state(&w).customization.column_drawers.is_empty());
        }
        let collapsed = state(&w).workspace;
        let gap = [
            column.bounds.x + column.bounds.width * 0.5,
            column.bounds.y + WORKSPACE_SPACING * 0.5,
        ];
        for point in [center(column.empty), gap, center(column.grip)] {
            assert_eq!(w.columns.background_at(&w, point), Some(column.id));
            double_click(point);
            let expanded = state(&w).workspace;
            assert!(!expanded.layout.is_collapsed(column.id));
            assert!(w.workspace_drag.borrow().is_none());
            assert!(!w.workspace_drag_input(ContactPhase::Move, [point[0] + 20., point[1]], None));
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            assert_eq!(
                serde_json::to_value(state(&w).workspace).unwrap(),
                serde_json::to_value(&collapsed).unwrap()
            );
            w.dispatch(UiAction::Invoke {
                command: CommandId::RedoWorkspace,
            });
            assert_eq!(
                serde_json::to_value(state(&w).workspace).unwrap(),
                serde_json::to_value(&expanded).unwrap()
            );
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(collapsed.clone()),
            });
            pump(250);
        }
    }
    for right in [false, true] {
        for collapsed in [false, true] {
            let mut workspace = initial.clone();
            for band in &mut workspace.layout.bands {
                if matches!(band.edge, Edge::Left | Edge::Right) {
                    band.edge = if (band.id == 3) == right {
                        Edge::Right
                    } else {
                        Edge::Left
                    };
                }
            }
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(workspace),
            });
            w.dispatch(UiAction::DoubleClickPanelHandle { group: 8, viewport });
            if collapsed {
                w.dispatch(UiAction::DoubleClickPanelHandle { group: 5, viewport });
            }
            pump(250);
            let before = state(&w).workspace;
            let r = w.resolved();
            let source = r.collapsed.iter().find(|c| c.id == 8).unwrap();
            let grip = source.grip;
            let start = [grip.x + grip.width * 0.5, grip.y + grip.height * 0.5];
            let divider = r.dividers.iter().find(|d| d.id == 3).unwrap();
            let x = if right {
                divider.bounds.x
            } else {
                divider.bounds.x + divider.bounds.width
            };
            let y = divider.bounds.y + divider.bounds.height * 0.5;
            let sign = if right { -1. } else { 1. };
            input.perform(serde_json::json!([{ "point": start }]));
            assert_eq!(cursor().as_deref(), Some("grab"));
            input.perform(serde_json::json!([
                {"point": start}, {"down": true}, {"point": [viewport[0] * 0.5, y]}
            ]));
            for distance in [90., 80., 60., 40., 20., 1., 0.] {
                input.perform(serde_json::json!([{"point": [x + sign * distance, y]}]));
                let hint = w.drop_hint.borrow().clone();
                if distance == 90. {
                    assert!(hint.is_none());
                } else {
                    let hint = hint.unwrap_or_else(|| panic!("missing native column hint: right={right}, collapsed={collapsed}, distance={distance}"));
                    assert_eq!(hint.bounds.height, divider.bounds.height);
                }
                assert_eq!(cursor().as_deref(), Some("grabbing"));
                assert_eq!(
                    state(&w).workspace,
                    before,
                    "drag keeps the column in place until release"
                );
            }
            input.perform(serde_json::json!([{"down": false}]));
            assert!(!matches!(cursor().as_deref(), Some("grabbing" | "no-drop")));
            let after = state(&w).workspace;
            assert_ne!(after, before);
            after.validate().unwrap();
            assert!(after.layout.floating.is_empty());
            assert!(after.layout.is_collapsed(8));
            assert_eq!(
                after.layout.group_panels(8).unwrap(),
                before.layout.group_panels(8).unwrap()
            );
            let r = w.resolved();
            let column = r.collapsed.iter().find(|c| c.id == 8).unwrap();
            let target = if collapsed {
                r.collapsed.iter().find(|c| c.id == 4).unwrap().bounds
            } else {
                r.groups.iter().find(|g| g.id == 5).unwrap().bounds
            };
            assert!(if right {
                column.bounds.x + column.bounds.width <= target.x
            } else {
                column.bounds.x >= target.x + target.width
            });
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            assert_eq!(state(&w).workspace, before);
            w.dispatch(UiAction::Invoke {
                command: CommandId::RedoWorkspace,
            });
            assert_eq!(state(&w).workspace, after);
        }
    }
    for blur in [false, true] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(initial.clone()),
        });
        w.dispatch(UiAction::DoubleClickPanelHandle { group: 8, viewport });
        pump(250);
        let before = state(&w).workspace;
        let grip = w
            .resolved()
            .collapsed
            .into_iter()
            .find(|c| c.id == 8)
            .unwrap()
            .grip;
        let start = [grip.x + grip.width * 0.5, grip.y + grip.height * 0.5];
        let away = [viewport[0] * 0.5, viewport[1] * 0.5];
        input.perform(serde_json::json!([{ "point": start }, { "down": true }, { "point": away }]));
        assert_eq!(cursor().as_deref(), Some("grabbing"));
        if blur {
            w.interact(UiInput::Blur);
        } else {
            w.workspace_drag_input(ContactPhase::Cancel, away, None);
        }
        pump(100);
        assert!(w.workspace_drag.borrow().is_none());
        assert!(!matches!(cursor().as_deref(), Some("grabbing" | "no-drop")));
        input.perform(serde_json::json!([{ "down": false }]));
        assert_eq!(state(&w).workspace, before);
    }
    input.finish();
    pump(100);
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_toolbar_drag_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("dev.layer.ToolbarDragInputTest");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    let target = if std::env::var_os("LAYER_NATIVE_DRAG_TAB").is_some() {
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Toolbar,
            viewport: [w.surface.width() as f32, w.surface.height() as f32],
            target: DockTarget::Tab {
                group: 5,
                index: None,
            },
        });
        pump(150);
        w.groups
            .borrow()
            .iter()
            .find(|g| g.id == 5)
            .unwrap()
            .tabs
            .iter()
            .find(|(p, _)| *p == Panel::Toolbar)
            .unwrap()
            .1
            .clone()
            .upcast::<gtk::Widget>()
    } else {
        find_css(&w.panel_widget(Panel::Toolbar), "panel-grip").unwrap()
    };
    let start = target
        .compute_point(&w.surface, &gtk::graphene::Point::new(10.0, 10.0))
        .unwrap();
    let middle = [
        w.surface.width() as f32 * 0.5,
        w.surface.height() as f32 * 0.5,
    ];
    let mut points = Vec::new();
    let mut from = [start.x(), start.y()];
    for to in [
        [middle[0], 260.0],
        middle,
        [middle[0] + 170.0, 120.0],
        [middle[0] - 100.0, middle[1] + 80.0],
        middle,
    ] {
        for i in 1..=12 {
            let t = i as f32 / 12.0;
            points.push([
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ]);
        }
        from = to;
    }
    std::fs::write(
        dir.join("ready"),
        serde_json::to_vec(&serde_json::json!({
            "start": [start.x(), start.y()], "points": points,
        }))
        .unwrap(),
    )
    .unwrap();
    let timeout = Instant::now() + Duration::from_secs(12);
    let mut floated = false;
    let mut cancelled = false;
    let mut states = Vec::new();
    while Instant::now() < timeout && !dir.join("finished").exists() {
        pump(10);
        let workspace = state(&w).workspace;
        let now_floating = !workspace.layout.floating.is_empty();
        cancelled |= floated && !now_floating;
        floated |= now_floating;
        states.push(workspace.layout.floating);
    }
    pump(200);
    capture_reference(&w, &dir.join("toolbar-drag.png").to_string_lossy(), 1.0);
    std::fs::write(
        dir.join("states.json"),
        serde_json::to_vec(&states).unwrap(),
    )
    .unwrap();
    assert!(dir.join("finished").exists(), "native driver timed out");
    assert!(floated, "real GTK input must tear the ribbon off");
    assert!(
        !cancelled,
        "floating toolbar reverted during a continuous native drag"
    );
    assert_eq!(state(&w).workspace.layout.floating.len(), 1);
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_compositor_input() {
    let report_dir = std::path::PathBuf::from(
        std::env::var("LAYER_NATIVE_INPUT_DIR")
            .expect("run with apps/layer-linux/bench/native-input.js in isolated Mutter"),
    );
    let app = native_test_app("dev.layer.NativeInputTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(1500);
    assert!(w.gpu.borrow().is_some());
    let events = Rc::new(RefCell::new(Vec::<[f64; 5]>::new()));
    let motion = gtk::EventControllerLegacy::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    motion.connect_event(glib::clone!(
        #[strong]
        events,
        move |_, event| {
            if event.event_type() == gdk::EventType::MotionNotify {
                let (x, y) = event.position().unwrap();
                events.borrow_mut().push([
                    glib::monotonic_time() as f64 * 1000.0,
                    event.time() as f64,
                    f64::from(
                        event
                            .modifier_state()
                            .contains(gdk::ModifierType::BUTTON2_MASK),
                    ),
                    x,
                    y,
                ]);
            }
            glib::Propagation::Proceed
        }
    ));
    w.area.add_controller(motion);
    let stats = ui_session(&w)
        .engine()
        .backend()
        .stats
        .clone();
    *stats.lock().unwrap() = Default::default();
    let before = state(&w).camera;
    let loop_ = glib::MainLoop::new(None, false);
    glib::timeout_add_local_once(
        Duration::from_secs(9),
        glib::clone!(
            #[strong]
            loop_,
            move || loop_.quit()
        ),
    );
    std::fs::write(report_dir.join("ready"), b"ready").unwrap();
    loop_.run();
    let stats = stats.lock().unwrap();
    let report = serde_json::json!({
        "events": &*events.borrow(), "input_cpu": stats.input_cpu, "input_handler_cpu": stats.input_handler_cpu,
        "wake_lateness": stats.wake_lateness, "worker_cpu": stats.cpu,
        "worker_cpu_stages": stats.cpu_stages, "worker_gpu": stats.gpu,
        "worker_thread_cpu": stats.thread_cpu,
        "frame_handler_cpu": stats.frame_handler_cpu,
        "canvas_presentation": stats.presented,
    });
    std::fs::write(
        report_dir.join("received.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    eprintln!(
        "Native delivery: {} pointer events; {} rendered frames",
        events.borrow().len(),
        stats.cpu.len()
    );
    assert_ne!(
        state(&w).camera,
        before,
        "native input must actually pan the canvas"
    );
    assert!(
        events.borrow().iter().filter(|e| e[2] == 1.0).count() > 720,
        "must receive compositor-originated panning events above 120 Hz"
    );
    drop(stats);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "hardware desktop: run this test separately with --ignored --test-threads=1"]
fn native_workspace_controls_docking_and_ink() {
    let app = native_test_app("dev.layer.CopilotTest");
    let w = fixture_workspace(&app);
    w.window.present();
    pump(3000);
    assert!(w.gpu.borrow().is_some());
    let restore_fixture_docking = || {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState {
                layout: DockLayout::default(),
                ..state(&w).workspace
            }),
        })
    };
    assert_eq!(state(&w).settings.theme, None);
    assert_eq!(
        state(&w).theme == Theme::Dark,
        adw::StyleManager::default().is_dark()
    );
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while w.frame_timer.borrow().is_some() && Instant::now() < deadline {
        pump(5);
    }
    assert!(w.frame_timer.borrow().is_none(), "idle canvas must stop requesting frames");
    let close = find_css(w.header.root.upcast_ref(), "close").unwrap();
    let circle = close.first_child().unwrap().compute_bounds(&close).unwrap();
    assert_eq!((circle.width(), circle.height()), (24.0, 24.0));
    let hit = close.compute_bounds(&w.header.root).unwrap();
    assert!(hit.width() >= 34.0 && hit.height() >= 34.0);
    let circle = close
        .first_child()
        .unwrap()
        .compute_bounds(&w.window)
        .unwrap();
    let right = w.window.width() as f32 - circle.x() - circle.width();
    assert!(
        (10.0..=14.0).contains(&circle.y()) && (10.0..=14.0).contains(&right),
        "native close insets: top={}, right={right}",
        circle.y()
    );
    // Exercise the same native signal callbacks as drawing. Space-pan uses
    // that path instead of a second recognizer competing for left-button input.
    let controllers = w.area.observe_controllers();
    let stylus = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::GestureStylus>())
        .unwrap();
    let before = state(&w).camera;
    w.interact(crate::input::key_input(
        gdk::Key::space,
        true,
        gdk::ModifierType::empty(),
        false,
        None,
    ));
    stylus.emit_by_name::<()>("down", &[&600.0f64, &450.0f64]);
    stylus.emit_by_name::<()>("motion", &[&640.0f64, &470.0f64]);
    w.interact(crate::input::key_input(
        gdk::Key::space,
        false,
        gdk::ModifierType::empty(),
        false,
        None,
    ));
    stylus.emit_by_name::<()>("motion", &[&660.0f64, &490.0f64]);
    stylus.emit_by_name::<()>("up", &[&660.0f64, &490.0f64]);
    pump(100);
    let dpi = w.area.scale_factor() as f32;
    assert_eq!(
        state(&w).camera.translation,
        [
            before.translation[0] + 60.0 * dpi,
            before.translation[1] + 40.0 * dpi
        ]
    );
    assert_eq!(
        ui_session(&w)
            .engine()
            .metrics()
            .committed_strokes,
        0
    );
    click(&command(&w, CommandId::FitCanvas));
    let initial = white_pixels(&w);
    assert!(initial > 100_000, "GPU paper should be visibly presented");
    w.dispatch(UiAction::Invoke {
        command: CommandId::Pencil,
    });
    let pencil = w
        .tool_set
        .buttons
        .borrow()
        .iter()
        .find(|(_, b, _)| b.widget_name() == "brush-2")
        .unwrap()
        .clone();
    click(&pencil.1);
    assert_eq!(Some(state(&w).brush.preset), pencil.0.preview);
    let size = w
        .size_buttons
        .borrow()
        .iter()
        .find(|(v, _)| *v == 96.0)
        .unwrap()
        .1
        .clone();
    click(&size);
    assert_eq!(state(&w).brush.diameter, 96.0);
    edit_number(&w.size_number, "84");
    assert_eq!(state(&w).brush.diameter, 84.0);
    assert_eq!(w.size_number.value(), 84.0);
    click(&command(&w, CommandId::AddLayer));
    assert_eq!(state(&w).layers.len(), 3);
    let view = state(&w).camera;
    let mut stroke_points = Vec::new();
    for i in 0..=36 {
        let t = i as f32 / 36.0;
        let phase = if i == 0 {
            PenPhase::Down
        } else if i == 36 {
            PenPhase::Up
        } else {
            PenPhase::Move
        };
        let event = PenEvent {
            device_id: 1,
            sequence: i + 1,
            timestamp_ns: glib::monotonic_time() as u64 * 1000,
            view_revision: view.revision,
            surface_position: Point {
                x: view.viewport[0] as f32 * (0.2 + 0.6 * t),
                y: view.viewport[1] as f32 * (0.5 + 0.09 * (t * std::f32::consts::TAU).sin()),
            },
            pressure: 0.3 + 0.7 * (t * std::f32::consts::PI).sin(),
            tilt_radians: [0.0; 2],
            twist_radians: 0.0,
            distance: 0.0,
            phase,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        };
        stroke_points.push(view.input_transform().map(event.surface_position));
        ui_session_mut(&w)
            .pen(event)
            .unwrap();
        w.wake();
        pump(15);
    }
    pump(300);
    let after_ink = white_pixels(&w);
    assert!(
        after_ink + 500 < initial,
        "ink must be visibly presented: {initial} -> {after_ink}"
    );
    assert_eq!(
        ui_session(&w)
            .engine()
            .metrics()
            .committed_strokes,
        1
    );
    assert_stroke_positions(&w, &crate::snapshot(&w), &stroke_points);
    // Presentation must follow the same top-left transform as input even
    // after an asymmetric pan/zoom/rotation (a centered fit hides a Y flip).
    let center = view.viewport.map(|v| v as f32 * 0.5);
    let change = ui_session_mut(&w).gesture(
        center,
        [center[0] + 30.0, center[1] + 45.0],
        1.1,
        0.35,
    );
    w.changed(change);
    pump(150);
    assert_stroke_positions(&w, &crate::snapshot(&w), &stroke_points);
    click(&command(&w, CommandId::FitCanvas));
    click(&command(&w, CommandId::Undo));
    assert!(white_pixels(&w) > after_ink + 500);
    click(&command(&w, CommandId::Redo));
    assert!(white_pixels(&w) < initial - 500);
    let visibility = || {
        named::<gtk::Box>(&w.layer_panel.root.clone().upcast(), &format!("art-layer-{}", state(&w).layers[0].id))
        .first_child()
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap()
    };
    visibility().emit_clicked();
    pump(100);
    assert!(white_pixels(&w) > after_ink + 500);
    visibility().emit_clicked();
    pump(100);
    assert!(white_pixels(&w) < initial - 500);
    edit_number(&w.layer_panel.opacity, "50");
    pump(100);
    assert_eq!(state(&w).layers[0].opacity, 0.5);
    edit_number(&w.layer_panel.opacity, "100");
    pump(100);
    let painted_id = state(&w).layers[0].id;
    w.dispatch(UiAction::Invoke {
        command: CommandId::LowerLayer,
    });
    assert_eq!(state(&w).layers[1].id, painted_id);
    w.dispatch(UiAction::Invoke {
        command: CommandId::RaiseLayer,
    });
    assert_eq!(state(&w).layers[0].id, painted_id);

    click(&command(&w, CommandId::Settings));
    assert!(state(&w).settings_open);
    assert!(w.preferences.dialog.root().is_some());
    let pressure: crate::number_control::NumberControl =
        find_named(w.preferences.dialog.upcast_ref(), "setting-pressure")
            .unwrap()
            .downcast()
            .unwrap();
    edit_number(&pressure, "1.45");
    assert_eq!(state(&w).settings.pressure_gamma, 1.45);
    w.preferences.dialog.close();
    pump(300);
    assert!(!state(&w).settings_open);
    assert_eq!(state(&w).settings.pressure_gamma, 1.45);
    click(&command(&w, CommandId::Settings));
    edit_number(&pressure, "1.5");
    let close = find_css(
        &find_named(w.preferences.dialog.upcast_ref(), "preferences-content").unwrap(),
        "close",
    )
    .unwrap()
    .downcast()
    .unwrap();
    click(&close);
    pump(300);
    assert_eq!(state(&w).settings.pressure_gamma, 1.5);
    assert!(!state(&w).settings_open);
    pump(300);

    restore_fixture_docking();
    pump(150);
    // Dock resizing must retain its native drag handle and GPU session.
    let handle = w
        .surface
        .imp()
        .children
        .borrow()
        .iter()
        .find(|(s, _)| *s == Slot::Divider(3))
        .unwrap()
        .1
        .clone();
    drag_divider(&w, 3, [282.0, 0.0]);
    pump(100);
    assert!(
        w.surface
            .imp()
            .children
            .borrow()
            .iter()
            .any(|(s, h)| *s == Slot::Divider(3) && *h == handle)
    );
    restore_fixture_docking();
    pump(100);
    // Repeated allocations retire differently sized display images. The pool
    // must remain bounded, eventually present the latest size, then go idle.
    for position in [250.0, 310.0, 260.0, 320.0, 270.0, 300.0] {
        drag_divider(&w, 3, [position, 0.0]);
        pump(25);
    }
    restore_fixture_docking();
    pump(200);
    let scale = w.area.scale_factor() as u32;
    assert_eq!(
        state(&w).camera.viewport,
        [
            w.area.width() as u32 * scale,
            w.area.height() as u32 * scale
        ]
    );
    assert!(w.frame_timer.borrow().is_none(), "resize presentation must settle");
    assert_eq!(
        ui_session(&w)
            .engine()
            .metrics()
            .committed_strokes,
        1
    );
    click(&command(&w, CommandId::FitCanvas));
    w.window.set_visible(false);
    pump(50);
    w.window.present();
    pump(200);
    std::fs::create_dir_all("../../artifacts/ui").unwrap();
    review(&w, "dark", &stroke_points);
    assert_eq!(
        crate::icons::name(
            &command(&w, CommandId::ZenMode)
                .child()
                .and_downcast::<gtk::Image>()
                .unwrap()
        )
        .as_deref(),
        Some("layer-zen-looking-up-symbolic")
    );
    let toolbar_bounds = w.toolbar.compute_bounds(&w.surface).unwrap();
    w.dispatch(UiAction::Invoke {
        command: CommandId::Brush,
    });
    pump(60);
    for pair in w.tool_set.buttons.borrow().windows(2).take(2) {
        let a = pair[0].1.compute_bounds(&w.surface).unwrap();
        let b = pair[1].1.compute_bounds(&w.surface).unwrap();
        assert_eq!(b.y() - a.y() - a.height(), 2.0);
    }
    assert_eq!(toolbar_bounds.y(), HEADER_HEIGHT);
    let zen = command(&w, CommandId::ZenMode)
        .compute_bounds(&w.surface)
        .unwrap();
    let below = toolbar_bounds.y() - zen.y() - zen.height();
    assert!(
        (below - zen.y()).abs() <= 2.0,
        "header control margins: above={}, below={below}",
        zen.y()
    );
    assert_eq!(toolbar_bounds.x(), w.resolved().status.x);
    assert_eq!(toolbar_bounds.width(), w.resolved().status.width);
    for group in w.groups.borrow().iter() {
        for (_, tab) in &group.tabs {
            assert_eq!(tab.compute_bounds(&w.surface).unwrap().height(), TILE_SIZE);
        }
    }
    // Window narrowing wraps the lone ribbon, without persisting its growth.
    w.window.set_default_size(680, 900);
    pump(300);
    assert_eq!(w.toolbar.height(), 74);
    let tiles = || {
        std::iter::successors(w.toolbar.first_child(), |c| c.next_sibling())
            .filter(|c| c.has_css_class("tile-button"))
            .map(|c| c.compute_bounds(&w.toolbar).unwrap())
            .collect::<Vec<_>>()
    };
    let wrapped = tiles();
    let a = wrapped[0];
    let b = wrapped.iter().find(|b| b.y() > a.y()).unwrap();
    assert_eq!(a.x(), b.x());
    assert_eq!(b.y() - a.y(), 38.0);
    crate::capture(&w, "../../artifacts/ui/gtk-tools-auto-wrap.png");
    w.window.set_default_size(1200, 900);
    pump(300);
    assert_eq!(w.toolbar.height(), 36);
    // One-column side ribbon, then two columns after cross-axis resizing.
    w.dispatch(UiAction::MovePanel {
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
        panel: Panel::Toolbar,
        target: DockTarget::Edge {
            edge: Edge::Left,
            outer: false,
        },
    });
    pump(100);
    assert_eq!(w.toolbar.width(), 36);
    let grip = w
        .toolbar
        .last_child()
        .unwrap()
        .compute_bounds(&w.toolbar)
        .unwrap();
    assert_eq!(
        (grip.width(), grip.height()),
        (w.toolbar.width() as f32, 20.0)
    );
    assert_eq!(grip.y() + grip.height(), w.toolbar.height() as f32);
    let layout = state(&w).workspace.layout;
    let band = layout.bands.last().unwrap();
    let d = w
        .resolved()
        .dividers
        .into_iter()
        .find(|d| d.id == band.id)
        .unwrap();
    drag_divider(
        &w,
        band.id,
        [d.bounds.x + d.bounds.width * 0.5 + 38.0, d.bounds.y],
    );
    pump(100);
    assert_eq!(w.toolbar.width(), 74);
    let wrapped = tiles();
    let a = wrapped[0];
    let b = wrapped.iter().find(|b| b.x() > a.x()).unwrap();
    assert_eq!(a.y(), b.y());
    assert_eq!(b.x() - a.x(), 38.0);
    crate::capture(&w, "../../artifacts/ui/gtk-tools-wrapped.png");
    restore_fixture_docking();
    pump(100);
    let camera = state(&w).camera;
    w.chrome_event(ChromeEvent::Motion {
        position: [600.0, 450.0],
    });
    click(&command(&w, CommandId::ZenMode));
    pump(250);
    // Native enter events from reparenting may arrive during the pump. This
    // assertion supplies its own hover location, independently of the desktop cursor.
    w.chrome_event(ChromeEvent::Motion {
        position: [600.0, 450.0],
    });
    w.update_zen();
    // Capture the settled 180ms fade, not the first frame after hiding chrome.
    pump(200);
    assert!(w.header.root.has_css_class("zen-hidden"));
    assert_eq!(state(&w).camera, camera);
    assert!(
        w.frame_timer.borrow().is_none(),
        "Zen fade must not run the canvas frame loop"
    );
    for (slot, widget) in w.surface.imp().children.borrow().iter() {
        if !matches!(slot, Slot::Canvas | Slot::CanvasBar) {
            assert!(!widget.can_target());
        }
    }
    crate::capture(&w, "../../artifacts/ui/gtk-zen.png");
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::Edit {
            id: PreferenceId::ZenRevealAtEdges,
            value: PreferenceValue::Bool(true),
        },
    });
    assert!(w.reveal_chrome_at(600.0, 24.0));
    assert!(!w.header.root.has_css_class("zen-hidden"));
    assert!(command(&w, CommandId::ZenMode).has_css_class("selected-tool"));
    pump(200);
    crate::capture(&w, "../../artifacts/ui/gtk-zen-controls.png");
    w.chrome_event(ChromeEvent::Motion {
        position: [600.0, 450.0],
    });
    click(&command(&w, CommandId::Settings));
    assert!(
        !w.header.root.has_css_class("zen-hidden"),
        "settings must pin chrome"
    );
    w.dispatch(UiAction::CloseSettings);
    pump(100);
    click(&command(&w, CommandId::ZenMode));
    assert!(!w.header.root.has_css_class("zen-hidden"));
    assert_eq!(state(&w).camera.translation, camera.translation);
    w.dispatch(UiAction::Invoke {
        command: CommandId::ToggleTheme,
    });
    pump(200);
    assert_eq!(state(&w).theme, Theme::Light);
    review(&w, "light", &stroke_points);
    let center = state(&w).camera.viewport.map(|v| v as f32 * 0.5);
    let change = ui_session_mut(&w)
        .gesture(center, center, 4.0, 0.0);
    w.changed(change);
    pump(150);
    let texture = crate::snapshot(&w);
    texture
        .save_to_png("../../artifacts/ui/gtk-zoom.png")
        .unwrap();
    let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut bytes, texture.width() as usize * 4);
    let header_item = |item: HeaderItem| {
        let id = state(&w)
            .workspace
            .layout
            .header
            .entries()
            .find(|e| e.item == item)
            .unwrap()
            .id;
        find_named(w.header.root.upcast_ref(), &format!("header-item-{id}"))
            .unwrap()
            .compute_bounds(&w.window)
            .unwrap()
    };
    let menus = header_item(HeaderItem::MenuLabels);
    let title = header_item(HeaderItem::DocumentTitle);
    let gap = (menus.x() + menus.width() + title.x()) * 0.5;
    // Between the live menus and centered document title, not on a menu button.
    let offset = (24 * texture.width() as usize + gap as usize) * 4;
    assert!(
        bytes[offset..offset + 3].iter().all(|c| *c > 245),
        "zoomed paper must show through the title bar"
    );
    w.window.destroy();
    pump(100);
    assert!(
        w.gpu
            .borrow()
            .as_ref()
            .is_none_or(|g| g.session.engine().backend().worker_is_joined())
    );
}

fn review(w: &Workspace, theme: &str, points: &[Point]) {
    // Save the exact texture that passed the ink assertion, without taking a
    // second scene snapshot between validation and artifact generation.
    let texture = crate::snapshot(w);
    assert_stroke_positions(w, &texture, points);
    // Sample the stroke itself: light-theme chrome is also almost white, so
    // whole-window white-pixel counts cannot compare across themes.
    let path = format!("../../artifacts/ui/gtk-{theme}.png");
    texture.save_to_png(&path).unwrap();
    assert_stroke_positions(w, &gdk::Texture::from_filename(&path).unwrap(), points);
}

fn assert_stroke_positions(w: &Workspace, texture: &gdk::Texture, points: &[Point]) {
    let camera = state(w).camera;
    let [a, b, c, d, tx, ty] = camera.document_to_surface();
    let origin = w.area.compute_bounds(&w.window).unwrap();
    let scale = w.area.scale_factor() as f32;
    let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut bytes, texture.width() as usize * 4);
    for fraction in 1..=3 {
        let point = points[points.len() * fraction / 4];
        let x = (origin.x() + (a * point.x + c * point.y + tx) / scale).round() as usize;
        let y = (origin.y() + (b * point.x + d * point.y + ty) / scale).round() as usize;
        let offset = (y * texture.width() as usize + x) * 4;
        assert!(
            bytes[offset] < 235 && bytes[offset + 1] < 235 && bytes[offset + 2] < 235,
            "committed stroke fraction {fraction}/4 must be visible at its input position: ({x}, {y})"
        );
    }
}

#[path = "workspace_switcher_tests.rs"]
mod workspace_switcher_tests;

fn widgets(root: &gtk::Widget) -> impl Iterator<Item = gtk::Widget> {
    let mut pending = vec![root.clone()];
    std::iter::from_fn(move || {
        let widget = pending.pop()?;
        let mut child = widget.last_child();
        while let Some(node) = child {
            child = node.prev_sibling();
            pending.push(node);
        }
        Some(widget)
    })
}

fn mapped_label(root: &gtk::Widget, text: &str) -> Option<gtk::Widget> {
    widgets(root).find(|widget| widget.is_mapped()
        && widget.downcast_ref::<gtk::Label>().is_some_and(|label| label.text() == text))
}

fn menu_button(root: &gtk::Widget, label: &str) -> Option<gtk::MenuButton> {
    widgets(root).filter_map(|widget| widget.downcast::<gtk::MenuButton>().ok())
        .find(|button| button.label().as_deref() == Some(label))
}

fn descendants<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Vec<T> {
    widgets(root).filter_map(|widget| widget.downcast().ok()).collect()
}

fn find_menu_item(root: &gtk::Widget, label: &str) -> Option<gtk::Widget> {
    widgets(root).find(|widget| widget.type_().name() == "GtkModelButton"
        && widget.property::<String>("text") == label)
}

fn find_button(root: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    widgets(root).filter_map(|widget| widget.downcast::<gtk::Button>().ok())
        .find(|button| button.label().as_deref() == Some(label))
}

fn named<T: IsA<gtk::Widget>>(root: &gtk::Widget, name: &str) -> T {
    find_named(root, name).and_then(|widget| widget.downcast().ok())
        .unwrap_or_else(|| panic!("{name}: expected {}", std::any::type_name::<T>()))
}

pub(crate) fn find_named(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    widgets(root).find_map(|widget| {
        if widget.widget_name() == "export-navigation" {
            let nav = widget.downcast_ref::<adw::NavigationView>().unwrap();
            for tag in ["main", "size", "color", "presets"] {
                if let Some(page) = nav.find_page(tag)
                    && let Some(found) = page.child().and_then(|child| find_named(&child, name)) {
                    return Some(found);
                }
            }
        }
        (widget.widget_name() == name).then_some(widget)
    })
}

fn descendant<T: IsA<gtk::Widget>>(root: &impl IsA<gtk::Widget>) -> Option<T> {
    widgets(root.as_ref()).find_map(|widget| widget.downcast().ok())
}

fn apply_dialog(w: &Workspace, name: &str, completed: bool) -> adw::AlertDialog {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        pump(if completed { 20 } else { 1 });
        if let Some(d) = w
            .window
            .visible_dialog()
            .filter(|d| d.widget_name() == name)
        {
            let d = d.downcast::<adw::AlertDialog>().unwrap();
            if !completed || d.is_response_enabled("apply") {
                return d;
            }
            let status = find_named(d.upcast_ref(), "color-preview-status").unwrap();
            let status = status.downcast::<gtk::Label>().unwrap().label();
            assert!(Instant::now() < deadline, "{name}: {status}");
        }
        assert!(Instant::now() < deadline, "{name}: {}", w.status.text());
    }
}

fn find_css(root: &gtk::Widget, class: &str) -> Option<gtk::Widget> {
    widgets(root).find(|widget| widget.has_css_class(class))
}

pub(crate) struct RemoteInput {
    dir: std::path::PathBuf,
    step: usize,
    settle_ms: u64,
    timeout: Duration,
}
impl RemoteInput {
    pub(crate) fn new() -> Self {
        Self {
            dir: std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap().into(),
            step: 0,
            settle_ms: 100,
            timeout: Duration::from_secs(5),
        }
    }
    fn settle_ms(self, settle_ms: u64) -> Self {
        Self { settle_ms, ..self }
    }
    fn timeout_secs(self, seconds: u64) -> Self {
        Self {
            timeout: Duration::from_secs(seconds),
            ..self
        }
    }
    pub(crate) fn ready(&self) {
        std::fs::write(self.dir.join("ready"), "ready").unwrap();
    }
    fn perform(&mut self, events: serde_json::Value) {
        let file = self.dir.join(format!("step-{}.json", self.step));
        let temporary = file.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(temporary, file).unwrap();
        let deadline = Instant::now() + self.timeout;
        while !self.dir.join(format!("done-{}", self.step)).exists() {
            assert!(Instant::now() < deadline, "native input step {}", self.step);
            pump(5);
        }
        self.step += 1;
        pump(self.settle_ms);
    }
    pub(crate) fn click(&mut self, point: [f32; 2]) {
        self.perform(serde_json::json!([{ "point": point }, { "down": true }, { "down": false }]));
    }
    fn key(&mut self, key: u32) {
        self.perform(
            serde_json::json!([{ "key": key, "down": true }, { "key": key, "down": false }]),
        );
    }
    pub(crate) fn finish(&self) {
        std::fs::write(self.dir.join("finished"), "done").unwrap();
    }
}

pub(crate) fn screen_point(widget: &gtk::Widget, window: &impl IsA<gtk::Widget>, at: [f32; 2]) -> [f32; 2] {
    let b = if let Some(popup) = widget.native().and_downcast::<gtk::Popover>() {
        let mut surface = popup.surface().unwrap();
        let (dx, dy) = popup.surface_transform();
        let (mut x, mut y) = (-dx as f32, -dy as f32);
        while let Ok(parent) = surface.clone().downcast::<gdk::Popup>() {
            x += parent.position_x() as f32;
            y += parent.position_y() as f32;
            surface = parent.parent().unwrap();
        }
        widget.compute_bounds(&popup).unwrap().offset_r(x, y)
    } else {
        widget.compute_bounds(window).unwrap()
    };
    [b.x() + b.width() * at[0], b.y() + b.height() * at[1]]
}

fn contact(device: &str, phase: &str, point: [f32; 2]) -> serde_json::Value {
    match (device, phase) {
        ("touch" | "pen", _) => serde_json::json!({ device: phase, "point": point }),
        (_, "down") => serde_json::json!({ "point": point, "down": true }),
        (_, "up") => serde_json::json!({ "down": false }),
        _ => serde_json::json!({ "point": point }),
    }
}

#[test]
#[ignore = "isolated Mutter pointer driver and SQLite; workspace-motion.sh gtk --workspace-menus"]
fn native_workspace_menu_input() {

    let mut input = RemoteInput::new().settle_ms(250);
    let dir = input.dir.clone();
    assert!(crate::storage::workspaces().is_some());
    let app = native_test_app("art.capycanvas.WorkspaceMenuInput");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    pump(500);
    assert!(w.window.is_maximized());
    input.ready();
    let mut click = |point: [f32; 2], button: u32| {
        input.perform(serde_json::json!([{ "point": point }, { "button": button, "down": true }, { "button": button, "down": false }]));
    };
    let manager = w.workspaces.manager().unwrap();
    assert_eq!(
        manager.active_id().as_deref(),
        Some(layer_workspace::DEFAULT_WORKSPACES[1].0)
    );
    let document = ui_session(&w)
        .engine()
        .document()
        .clone();
    w.dispatch(UiAction::SetBrushSize { value: 37. });
    for (visit, index) in [0, 2, 1, 0, 1].into_iter().enumerate() {
        let (id, preset) = layer_workspace::DEFAULT_WORKSPACES[index];
        let button = find_named(
            w.header.root.upcast_ref(),
            &format!("workspace-switch-{}", id.rsplit(':').next().unwrap()),
        )
        .unwrap();
        let bounds = button.compute_bounds(&w.window).unwrap();
        click(
            [
                bounds.x() + bounds.width() / 2.,
                bounds.y() + bounds.height() / 2.,
            ],
            272,
        );
        until(
            || w.workspaces.switcher.is_sensitive(),
            "workspace switch timed out",
        );
        assert_eq!(manager.active_id().as_deref(), Some(id));
        assert!(
            button
                .downcast_ref::<gtk::ToggleButton>()
                .unwrap()
                .is_active()
        );
        assert_eq!(
            durable_layout(&state(&w).workspace.layout),
            preset.layout(Platform::Gtk)
        );
        assert_eq!(
            ui_session(&w).engine().document(),
            &document
        );
        capture_reference(
            &w,
            dir.join(format!("{}.png", preset.name())).to_str().unwrap(),
            1.,
        );
        if index == 0 {
            if visit > 0 {
                assert_eq!(state(&w).brush.diameter, 73.);
            }
            // Window-bar drawers remain reachable with no docked panels.
            let layout = state(&w).workspace.layout;
            let drawers: Vec<_> = layout
                .header
                .entries()
                .filter_map(|e| match e.item {
                    HeaderItem::Tool {
                        control: ToolbarControl::Panel { panel },
                    } => Some((e.id, panel)),
                    _ => None,
                })
                .collect();
            assert!(drawers.iter().any(|(_, panel)| *panel == Panel::Layers));
            for (id, panel) in drawers {
                let button =
                    find_named(w.header.root.upcast_ref(), &format!("header-item-{id}")).unwrap();
                let bounds = button.compute_bounds(&w.window).unwrap();
                let point = [
                    bounds.x() + bounds.width() / 2.,
                    bounds.y() + bounds.height() / 2.,
                ];
                click(point, 272);
                assert!(
                    state(&w).customization.drawer.is_some(),
                    "Painter {panel:?} drawer"
                );
                if panel == Panel::Layers {
                    capture_reference(&w, dir.join("Painter-Layers.png").to_str().unwrap(), 1.);
                }
                click(point, 272);
                assert!(state(&w).customization.drawer.is_none());
                assert_eq!(
                    durable_layout(&state(&w).workspace.layout),
                    durable_layout(&layout)
                );
            }
            w.dispatch(UiAction::SetBrushSize { value: 73. });
        } else if index == 1 {
            assert_eq!(state(&w).brush.diameter, 37.);
        }
    }
    for label in ["Window", "File"] {
        let button = menu_button(w.header.root.upcast_ref(), label).unwrap();
        let bounds = button.compute_bounds(&w.window).unwrap();
        let popup = button.popover().unwrap();
        click(
            [
                bounds.x() + bounds.width() * 0.5,
                bounds.y() + bounds.height() * 0.5,
            ],
            272,
        );
        assert!(
            popup.is_visible(),
            "{label} menu did not remain open after a native click; sensitive={}, ready={}, busy={}, storage={:?}",
            w.surface.is_sensitive(),
            w.workspaces.ready(),
            w.workspaces.busy(),
            w.workspaces.manager().unwrap().error()
        );
        assert!(popup.is_mapped());
        capture_popover(&popup, dir.join(format!("{label}.png")).to_str().unwrap());
        if label == "Window" {
            assert!(mapped_label(popup.upcast_ref(), "Layers").is_some());
            let toolbars = mapped_label(popup.upcast_ref(), "Quick Access Toolbars").unwrap();
            click(screen_point(&toolbars, &w.window, [0.5, 0.5]), 272);
            assert!(mapped_label(popup.upcast_ref(), "Tools").is_some());
            capture_popover(
                &popup,
                dir.join("Quick-Access-Toolbars.png").to_str().unwrap(),
            );
            popup
                .downcast_ref::<gtk::PopoverMenu>()
                .unwrap()
                .set_visible_submenu(Some("main"));
            pump(100);
            let workspace = mapped_label(popup.upcast_ref(), "Workspaces").unwrap();
            click(screen_point(&workspace, &w.window, [0.5, 0.5]), 272);
            let manage = widgets(popup.upcast_ref()).find(|widget| widget.is_mapped()
                && widget.type_().name() == "GtkModelButton"
                && widget.property::<Option<gtk::PopoverMenu>>("popover").is_none()
                && descendant::<gtk::Label>(widget).is_some_and(|label| label.label() == w.localization().text(MessageId::WORKSPACE_WORKSPACES).as_ref()))
                .expect("Workspace manager action should open");
            capture_popover(&popup, dir.join("Workspace.png").to_str().unwrap());
            click(screen_point(&manage, &w.window, [0.5, 0.5]), 272);
            assert!(
                w.workspaces.ui.dialog.is_visible(),
                "Manage Workspaces should open through the native menu"
            );
            let plus = find_named(w.window.upcast_ref(), "workspace-manager-new").unwrap();
            let plus_bounds = plus.compute_bounds(&w.window).unwrap();
            click(
                [
                    plus_bounds.x() + plus_bounds.width() / 2.,
                    plus_bounds.y() + plus_bounds.height() / 2.,
                ],
                272,
            );
            assert!(find_named(w.window.upcast_ref(), "workspace-start-with").is_none());
            let entry = find_named(w.window.upcast_ref(), "workspace-item-name").unwrap();
            let prompt = entry.ancestor(adw::AlertDialog::static_type()).unwrap();
            assert_eq!(
                prompt
                    .downcast_ref::<adw::AlertDialog>()
                    .unwrap()
                    .heading()
                    .as_deref(),
                Some("New Workspace")
            );
            let cancel = find_button(&prompt, "Cancel")
                .unwrap()
                .compute_bounds(&w.window)
                .unwrap();
            click(
                [
                    cancel.x() + cancel.width() / 2.,
                    cancel.y() + cancel.height() / 2.,
                ],
                272,
            );
            w.workspaces.ui.close(&w);
            pump(250);
            click(
                [
                    bounds.x() + bounds.width() * 0.5,
                    bounds.y() + bounds.height() * 0.5,
                ],
                272,
            );
            let workspace = mapped_label(popup.upcast_ref(), "Workspaces").unwrap();
            click(screen_point(&workspace, &w.window, [0.5, 0.5]), 272);
            let reset = mapped_label(popup.upcast_ref(), "Reset All Brushes…").unwrap();
            click(screen_point(&reset, &w.window, [0.5, 0.5]), 272);
            let button = find_button(w.window.upcast_ref(), "Reset Brushes").unwrap();
            capture_reference(&w, dir.join("Reset-Brushes.png").to_str().unwrap(), 1.);
            let bounds = button.compute_bounds(&w.window).unwrap();
            click(
                [
                    bounds.x() + bounds.width() / 2.,
                    bounds.y() + bounds.height() / 2.,
                ],
                272,
            );
            until(|| !w.workspaces.busy(), "workspace switch");
            assert!(
                manager
                    .editing()
                    .unwrap()
                    .tools
                    .overrides
                    .is_empty()
            );
            assert_eq!(
                state(&w).brush.diameter,
                layer_core::default_brush(layer_core::DefaultBrushPreset::GPen).diameter
            );
            pump(200);
        }
        popup.popdown();
        pump(200);
    }
    let tab = w
        .tab_hits()
        .into_iter()
        .find(|t| {
            state(&w)
                .workspace
                .layout
                .group_panels(t.group)
                .is_ok_and(|panels| panels.get(t.index) == Some(&Panel::Layers))
        })
        .unwrap();
    click(
        [
            tab.bounds.x + tab.bounds.width * 0.5,
            tab.bounds.y + tab.bounds.height * 0.5,
        ],
        273,
    );
    let context = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .find(|p| p.has_css_class("panel-context-menu") && p.is_visible())
        .expect("The panel context menu should remain open after a native right click");
    capture_popover(&context, dir.join("context.png").to_str().unwrap());
    w.dismiss_context();
    input.finish();
    w.window.close();
    pump(300);
}

#[test]
#[ignore = "private Wayland display, Vulkan and native workspace storage: native SQLite workspace resume"]
fn native_workspace_database_resume_and_independent_windows() {
    assert!(
        crate::storage::workspaces().is_some(),
        "Run with --native-storage"
    );
    let app = native_test_app("art.capycanvas.WorkspacePersistence");
    let wait_saved = |w: &Workspace| {
        let deadline = Instant::now() + Duration::from_secs(10);
        let manager = w.workspaces.manager().unwrap();
        while manager.dirty() || manager.saving() {
            pump(20);
            assert!(
                manager.error().is_none(),
                "workspace save: {:?}",
                manager.error()
            );
            assert!(Instant::now() < deadline, "workspace save deadline");
        }
        assert!(manager.error().is_none());
    };
    let w = Workspace::new(&app);
    w.window.present();
    wait_workspaces(&w);
    let id = w.workspaces.manager().unwrap().active_id().unwrap();
    let baseline = durable_layout(&state(&w).workspace.layout);
    let document = ui_session(&w)
        .engine()
        .document()
        .clone();
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Layers,
        target: DockTarget::Float {
            position: [420., 180.],
        },
        viewport: [1200., 900.],
    });
    w.dispatch(UiAction::SetBrushSize { value: 73. });
    w.dispatch(UiAction::SetColor {
        rgba: [0.2, 0.4, 0.6, 1.],
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    let moved = durable_layout(&state(&w).workspace.layout);
    assert_ne!(moved, baseline);
    wait_saved(&w);
    assert_eq!(
        ui_session(&w).engine().document(),
        &document
    );
    w.window.close();
    until(|| !w.window.is_visible(), "acknowledged close");
    drop(w);
    pump(30);
    let reopened = Workspace::new(&app);
    reopened.window.present();
    wait_workspaces(&reopened);
    assert_eq!(
        reopened
            .workspaces
            .manager()
            .unwrap()
            .active_id()
            .unwrap(),
        id
    );
    assert_eq!(durable_layout(&state(&reopened).workspace.layout), moved);
    assert_eq!(state(&reopened).brush.diameter, 73.);
    assert_eq!(state(&reopened).colors.foreground.rgba, [0.2, 0.4, 0.6, 1.]);
    assert!(state(&reopened).workspace.zen_mode);
    reopened.dispatch(UiAction::Invoke {
        command: CommandId::UndoWorkspace,
    });
    assert_eq!(durable_layout(&state(&reopened).workspace.layout), baseline);
    assert_eq!(state(&reopened).brush.diameter, 73.);
    assert!(state(&reopened).workspace.zen_mode);
    wait_saved(&reopened);
    reopened.window.close();
    until(|| !reopened.window.is_visible(), "reopened window close");
    drop(reopened);
    pump(30);
    let again = Workspace::new(&app);
    again.window.present();
    wait_workspaces(&again);
    again.dispatch(UiAction::Invoke {
        command: CommandId::RedoWorkspace,
    });
    assert_eq!(durable_layout(&state(&again).workspace.layout), moved);
    assert_eq!(state(&again).brush.diameter, 73.);
    wait_saved(&again);
    let second = Workspace::new(&app);
    second.window.present();
    wait_workspaces(&second);
    assert_ne!(
        again.workspaces.manager().unwrap().active_id(),
        second.workspaces.manager().unwrap().active_id()
    );
    let second_manager = second.workspaces.manager().unwrap();
    assert!(second_manager.current().unwrap().metadata.builtin);
    assert_eq!(
        second_manager.items().len(),
        3,
        "Opening a second window must reuse a built-in workspace"
    );
    second.dispatch(UiAction::SetBrushSize { value: 121. });
    wait_saved(&second);
    assert_eq!(state(&again).brush.diameter, 73.);
    assert_eq!(state(&second).brush.diameter, 121.);
    crate::capture(&second, "/tmp/capy-workspace-persistence.png");
    second.window.close();
    again.window.close();
    pump(500);
}

#[test]
#[ignore = "requires native workspace storage and a native GTK/Vulkan display"]
fn native_named_workspace_manager_library_and_history() {
    use layer_workspace::{ItemKind, ManagerAction as A, ManagerPage};
    assert!(crate::storage::workspaces().is_some());
    let app = native_test_app("art.capycanvas.NamedWorkspaceManager");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.present();
    wait_workspaces(&w);
    let manager = w.workspaces.manager().unwrap();
    let run = |action: A, name: Option<&str>, confirm: Option<&str>| {
        if name.is_some() || confirm.is_some() {
            let w = w.clone();
            let name = name.map(str::to_owned);
            let confirm = confirm.map(str::to_owned);
            let timeout = Instant::now() + Duration::from_secs(10);
            glib::timeout_add_local(Duration::from_millis(20), move || {
                assert!(Instant::now() < timeout, "manager prompt did not appear");
                if let Some(name) = &name {
                    let Some(entry) = find_named(w.window.upcast_ref(), "workspace-item-name")
                    else {
                        return glib::ControlFlow::Continue;
                    };
                    entry.downcast::<gtk::Entry>().unwrap().set_text(name);
                }
                if let Some(confirm) = &confirm {
                    let Some(button) = w
                        .window
                        .visible_dialog()
                        .and_then(|dialog| find_button(dialog.upcast_ref(), confirm))
                    else {
                        return glib::ControlFlow::Continue;
                    };
                    // Match a real confirmation click: move focus off the name
                    // field before closing it, then allow its input context to
                    // process focus-out before the dialog destroys the field.
                    button.grab_focus();
                    glib::timeout_add_local_once(Duration::from_millis(100), move || {
                        button.emit_clicked();
                    });
                }
                glib::ControlFlow::Break
            });
        }
        perform(&w, action);
        pump(100);
        assert!(!w.workspaces.busy());
    };
    let drawing = ui_session(&w)
        .engine()
        .document()
        .clone();
    run(A::New, Some("Painting"), Some("Create and Switch"));
    assert_eq!(manager.active_name().as_deref(), Some("Painting"));
    let painting = manager.active_id().unwrap();
    assert!(manager.switcher_ids().contains(&painting));
    assert!(
        find_named(
            w.header.root.upcast_ref(),
            &format!("workspace-switch-{painting}")
        )
        .is_some()
    );
    let baseline = durable_layout(&state(&w).workspace.layout);
    w.dispatch(UiAction::SetBrushSize { value: 73. });
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Layers,
        target: DockTarget::Float {
            position: [430., 170.],
        },
        viewport: [1200., 900.],
    });
    let customized = durable_layout(&state(&w).workspace.layout);
    let manager_list = || {
        named::<gtk::ListBox>(w.window.upcast_ref(), "workspace-manager-items")
    };
    let select_item = |title: &str| {
        let list = manager_list();
        let mut child = list.first_child();
        loop {
            let row = child.expect("The workspace item should be listed");
            if row
                .downcast_ref::<adw::ActionRow>()
                .is_some_and(|row| row.title() == title)
            {
                let row = row.downcast::<gtk::ListBoxRow>().unwrap();
                list.select_row(Some(&row));
                // Row activation (including a double click or Enter) only selects.
                list.emit_by_name::<()>("row-activated", &[&row]);
                break;
            }
            child = row.next_sibling();
        }
    };
    let capture = || {
        ui_session_mut(&w)
            .capture_workspace()
            .unwrap()
    };
    let saved = |id: &str| {
        glib::MainContext::default()
            .block_on(manager.load(id))
            .unwrap()
            .entity
            .capture()
            .unwrap()
    };
    run(A::New, Some("Inking"), Some("Create and Switch"));
    assert_eq!(manager.active_name().as_deref(), Some("Inking"));
    assert_eq!(durable_layout(&state(&w).workspace.layout), customized);
    assert_eq!(state(&w).brush.diameter, 73.);
    let inking = manager.active_id().unwrap();
    assert!(manager.switcher_ids().contains(&inking));
    assert!(
        manager.switcher_ids().contains(&painting),
        "new workspace stays pinned after switching away"
    );
    w.dispatch(UiAction::SetBrushSize { value: 31. });
    w.customize(CustomizationAction::SetPanelVisible {
        panel: Panel::Color,
        visible: false,
    });
    let inking_layout = durable_layout(&state(&w).workspace.layout);
    run(A::Switch(painting.clone()), None, None);
    assert_eq!(state(&w).brush.diameter, 73.);
    let painting_before_preview = capture();
    let inking_before_preview = saved(&inking);
    for commit in [false, true] {
        w.workspaces.ui.show(&w, ManagerPage::Workspaces);
        pump(200);
        let apply = find_button(w.workspaces.ui.dialog.upcast_ref(), "Switch to Workspace").unwrap();
        assert!(!apply.is_sensitive());
        select_item("Inking");
        pump(150);
        assert!(apply.is_sensitive());
        assert_eq!(manager.active_id().as_deref(), Some(painting.as_str()));
        assert_eq!(durable_layout(&state(&w).workspace.layout), inking_layout);
        assert_eq!(capture(), painting_before_preview);
        assert_eq!(saved(&painting), painting_before_preview);
        assert_eq!(saved(&inking), inking_before_preview);
        crate::capture(&w, "/tmp/capy-workspace-manager-preview.png");
        if commit {
            apply.emit_clicked();
        } else {
            // Workspace previews retain the same cancellation and stale-load
            // protection after removing the separate saved-layout picker.
            select_item("Painting");
            select_item("Inking");
            select_item("Painting");
            pump(150);
            assert_eq!(
                durable_layout(&state(&w).workspace.layout),
                *painting_before_preview.history.layout()
            );
            select_item("Inking");
            let search = named::<gtk::SearchEntry>(w.window.upcast_ref(), "workspace-manager-search");
            search.set_text("no matching item");
            pump(300);
            assert!(manager_list().selected_row().is_none());
            assert!(!apply.is_sensitive());
            assert_eq!(
                durable_layout(&state(&w).workspace.layout),
                *painting_before_preview.history.layout()
            );
            search.set_text("");
            pump(300);
            select_item("Inking");
            find_button(w.workspaces.ui.dialog.upcast_ref(), "Cancel")
                .unwrap()
                .emit_clicked();
        }
        until(
            || !(w.workspaces.ui.dialog.is_mapped() || w.workspaces.busy()),
            "Workspace selection did not finish",
        );
        if commit {
            assert_eq!(manager.active_id().as_deref(), Some(inking.as_str()));
            assert_eq!(capture(), inking_before_preview);
            assert_eq!(state(&w).brush.diameter, 31.);
        } else {
            assert_eq!(manager.active_id().as_deref(), Some(painting.as_str()));
            assert_eq!(
                durable_layout(&state(&w).workspace.layout),
                *painting_before_preview.history.layout()
            );
            assert_eq!(capture(), painting_before_preview);
        }
    }
    run(A::Switch(painting.clone()), None, None);
    run(A::Reset(painting.clone()), None, Some("Restore"));
    assert_eq!(durable_layout(&state(&w).workspace.layout), baseline);
    assert_eq!(state(&w).brush.diameter, 73.);
    w.dispatch(UiAction::Invoke {
        command: CommandId::UndoWorkspace,
    });
    assert_eq!(durable_layout(&state(&w).workspace.layout), customized);
    let toolbar = state(&w)
        .workspace
        .layout
        .panels
        .iter()
        .find(|p| p.id.kind() == PanelKind::Tiles)
        .unwrap()
        .id;
    run(
        A::SaveToolbar(toolbar),
        Some("Ink Tools"),
        Some("Save to Library"),
    );
    let library = manager
        .items()
        .into_iter()
        .find(|i| i.metadata.kind == ItemKind::Toolbar && i.metadata.name == "Ink Tools")
        .unwrap()
        .id;
    let count = state(&w).workspace.layout.panels.len();
    run(A::AddToolbar(library.clone()), None, None);
    assert_eq!(state(&w).workspace.layout.panels.len(), count + 1);
    assert_eq!(state(&w).brush.diameter, 73.);
    run(A::Delete(library.clone()), None, Some("Delete"));
    assert_eq!(state(&w).workspace.layout.panels.len(), count + 1);
    assert!(
        glib::MainContext::default()
            .block_on(manager.load(&library))
            .is_err()
    );
    // Exercise the actual modal and check both live and persisted state while browsing.
    let original = ui_session_mut(&w)
        .capture_workspace()
        .unwrap();
    let persisted_original = glib::MainContext::default()
        .block_on(manager.load(&painting))
        .unwrap()
        .entity
        .capture()
        .unwrap();
    assert_eq!(
        persisted_original.history.generation,
        original.history.generation
    );
    let open_history = || {
        w.workspaces.send(
            &w,
            layer_workspace::WorkspaceInput::Action {
                action: A::History(painting.clone()),
            },
        );
        until(
            || {
                w.workspaces.view().page == Some(ManagerPage::History)
                    && !w.workspaces.busy()
                    && w.workspaces.ui.dialog.is_mapped()
            },
            "History page did not open",
        );
        pump(100);
        let dialog = w.workspaces.ui.dialog.clone();
        let list = named::<gtk::ListBox>(dialog.upcast_ref(), "workspace-manager-items");
        let apply = named::<gtk::Button>(dialog.upcast_ref(), "workspace-manager-apply");
        (dialog, list, apply)
    };
    for response in ["cancel", "restore"] {
        let (dialog, list, apply) = open_history();
        let before = glib::MainContext::default()
            .block_on(manager.load(&painting))
            .unwrap()
            .entity
            .capture()
            .unwrap();
        assert_eq!(before, persisted_original);
        assert!(!apply.is_sensitive());
        let current_row = list.selected_row().unwrap();
        let starting = list
            .last_child()
            .unwrap()
            .downcast::<gtk::ListBoxRow>()
            .unwrap();
        list.select_row(Some(&starting));
        pump(150);
        assert_eq!(durable_layout(&state(&w).workspace.layout), baseline);
        assert_eq!(
            ui_session_mut(&w)
                .capture_workspace()
                .unwrap(),
            original
        );
        assert_eq!(
            glib::MainContext::default()
                .block_on(manager.load(&painting))
                .unwrap()
                .entity
                .capture()
                .unwrap(),
            persisted_original
        );
        list.select_row(Some(&current_row));
        pump(50);
        assert_eq!(
            durable_layout(&state(&w).workspace.layout),
            *original.history.layout()
        );
        assert!(!apply.is_sensitive());
        list.select_row(Some(&starting));
        pump(100);
        assert!(apply.is_sensitive());
        if response == "cancel" {
            let expires = manager
                .current_record()
                .unwrap()
                .claim
                .unwrap()
                .expires_at_ms;
            pump(layer_workspace::OWNER_RENEW_MS + 200);
            let renewed = glib::MainContext::default()
                .block_on(manager.load(&painting))
                .unwrap();
            assert!(
                renewed.claim.as_ref().unwrap().expires_at_ms > expires,
                "History browsing must keep this workspace claimed"
            );
            assert_eq!(renewed.entity.capture().unwrap(), persisted_original);
        }
        crate::capture(&w, "/tmp/capy-workspace-layout-history.png");
        if response == "restore" {
            apply.emit_clicked();
        } else {
            find_button(dialog.upcast_ref(), "Cancel")
                .unwrap()
                .emit_clicked();
        }
        until(
            || w.workspaces.view().page.is_none() && !w.workspaces.busy(),
            "workspace operation",
        );
        pump(100);
        assert!(!w.workspaces.busy());
        let after = ui_session_mut(&w)
            .capture_workspace()
            .unwrap();
        if response == "cancel" {
            assert_eq!(after, original);
            assert_eq!(
                durable_layout(&state(&w).workspace.layout),
                *original.history.layout()
            );
        } else {
            assert_eq!(after.history.layout(), &baseline);
            assert_eq!(
                after.history.revisions.len(),
                original.history.revisions.len() + 1
            );
            assert_eq!(after.history.generation, original.history.generation + 1);
            assert_eq!(after.working, original.working);
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            assert_eq!(
                durable_layout(&state(&w).workspace.layout),
                *original.history.layout()
            );
        }
    }
    until(|| saved(&painting) == capture(), "workspace autosave");
    w.workspaces.ui.show(&w, ManagerPage::Workspaces);
    pump(200);
    let details = find_named(w.window.upcast_ref(), "workspace-manager-details");
    assert!(
        details.is_none_or(|widget| !widget.is_mapped()),
        "Workspace list should have no details pane"
    );
    crate::capture(&w, "/tmp/capy-workspace-manager-simple.png");
    assert!(find_button(w.window.upcast_ref(), "Switch to Workspace").is_some());
    assert!(find_named(w.window.upcast_ref(), "workspace-manager-new").is_some());
    assert_eq!(
        ui_session(&w).engine().document(),
        &drawing
    );
    let saved_before_close = saved(&painting);
    select_item("Inking");
    until(|| !w.workspaces.view().loading, "Inking preview");
    assert!(w.workspaces.view().selected.is_some());
    assert_ne!(
        durable_layout(&state(&w).workspace.layout),
        *saved_before_close.history.layout()
    );
    // GTK dismisses the modal on the first window-close action. A second
    // request closes the application, preserving the original saved layout.
    w.window.close();
    pump(200);
    assert!(!w.workspaces.ui.dialog.is_mapped());
    assert!(!w.workspaces.busy());
    assert_eq!(
        durable_layout(&state(&w).workspace.layout),
        *saved_before_close.history.layout()
    );
    w.window.close();
    until(|| !w.window.is_visible(), "Window close did not finish");
    assert_eq!(saved(&painting), saved_before_close);
}

#[test]
#[ignore = "isolated Wayland and private storage"]
fn native_workspace_unavailable_close_recovery() {
    let blocked = crate::storage::roots().unwrap().workspaces();
    std::fs::create_dir_all(blocked.parent().unwrap()).unwrap();
    std::fs::write(&blocked, b"not a folder").unwrap();
    let original = std::fs::read(&blocked).unwrap();
    let app = native_test_app("art.capycanvas.WorkspaceUnavailable");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.present();
    until(
        || !w.workspaces.busy() && w.workspaces.view().error.is_some(),
        "startup error was not presented",
    );
    assert!(!w.workspaces.ready());
    let diameter = state(&w).brush.diameter;
    w.dispatch(UiAction::SetBrushSize { value: 83. });
    assert_eq!(state(&w).brush.diameter, diameter);
    w.window.close();
    pump(100);
    let dialog = find_named(w.window.upcast_ref(), "workspace-close-recovery").unwrap();
    find_button(&dialog, "Keep Open").unwrap().emit_clicked();
    pump(100);
    assert!(w.window.is_visible());
    assert_eq!(state(&w).brush.diameter, diameter);
    assert!(!state(&w).document_file.close_ready);
    w.window.close();
    pump(100);
    let dialog = find_named(w.window.upcast_ref(), "workspace-close-recovery").unwrap();
    find_button(&dialog, "Discard Unsaved Changes")
        .unwrap()
        .emit_clicked();
    pump(200);
    assert!(!w.window.is_visible());
    assert_eq!(std::fs::read(blocked).unwrap(), original);
}

#[test]
#[ignore = "requires native workspace storage and a native GTK/Vulkan display"]
fn native_workspace_owner_takeover_preserves_recovery_and_blocks_stale_input() {
    use layer_workspace::ManagerAction as A;
    assert!(crate::storage::workspaces().is_some());
    let app = native_test_app("art.capycanvas.WorkspaceOwnership");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let first = Workspace::new(&app);
    first.window.present();
    wait_workspaces(&first);
    first.dispatch(UiAction::SetBrushSize { value: 73. });
    let manager = first.workspaces.manager().unwrap();
    let id = manager.active_id().unwrap();
    first
        .workspaces
        .send(&first, layer_workspace::WorkspaceInput::Suspend);
    until(
        || !manager.lease_valid(crate::workspace::manager::now_ms()),
        "suspended lease release",
    );
    let second = Workspace::new(&app);
    second.window.present();
    wait_workspaces(&second);
    assert_eq!(
        second.workspaces.manager().unwrap().active_id(),
        Some(id.clone())
    );
    first
        .workspaces
        .send(&first, layer_workspace::WorkspaceInput::Resume);
    until(|| first.workspaces.view().owner_lost, "ownership loss");
    assert!(!first.workspaces.accepts_input(&first));
    first.dispatch(UiAction::SetBrushSize { value: 119. });
    assert_eq!(state(&first).brush.diameter, 73.);
    assert_eq!(state(&second).brush.diameter, 73.);
    let window = first.clone();
    glib::timeout_add_local_once(Duration::from_millis(100), move || {
        named::<gtk::Entry>(window.window.upcast_ref(), "workspace-item-name")
            .set_text("Recovered Painting");
        find_button(window.window.upcast_ref(), "Save and Switch")
            .unwrap()
            .emit_clicked();
    });
    perform(&first, A::SaveAsNew);
    pump(100);
    assert_ne!(manager.active_id(), Some(id));
    assert_eq!(manager.active_name().as_deref(), Some("Recovered Painting"));
    first.dispatch(UiAction::SetBrushSize { value: 119. });
    assert_eq!(state(&first).brush.diameter, 119.);
    assert_eq!(state(&second).brush.diameter, 73.);
    first.window.close();
    second.window.close();
    pump(500);
    assert!(!first.window.is_visible() && !second.window.is_visible());
}

async fn read_canvas_pixels(
    w: &Rc<Workspace>,
    id: u32,
) -> Result<layer_render::ReadbackImage, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    // The ordered worker queue must receive the final document frame before
    // its readback. Merely changing the model does not update GPU pixels.
    loop {
        let pending = w
            .gpu
            .borrow()
            .as_ref()
            .ok_or("Canvas unavailable")?
            .session
            .engine()
            .has_pending_document_edits();
        if !pending {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err("The canvas is not ready to export".into());
        }
        w.wake();
        glib::timeout_future(std::time::Duration::from_millis(16)).await;
    }
    let mut gpu = w.gpu.borrow_mut();
    let renderer = gpu
        .as_mut()
        .ok_or("Canvas unavailable")?
        .session
        .renderer_mut();
    renderer.ready()?;
    renderer.document_pixels(u64::from(id))
}

#[path = "source_repair_tests.rs"]
mod source_repair;

#[path = "source_rasterize_tests.rs"]
mod source_rasterize;

#[path = "document_color_tests.rs"]
mod document_color;

#[path = "histogram_tests.rs"]
mod histogram;

#[path = "tonal_tests.rs"]
mod tonal;

#[path = "pointwise_tests.rs"]
mod pointwise;

#[path = "color_preferences_tests.rs"]
mod color_preferences;

#[path = "managed_view_tests.rs"]
mod managed_view;
// Compact-drawer fixtures opt in; new stacks otherwise open full columns.
fn enable_individual_column_panels(w: &Rc<Workspace>, group: u32) {
    let column = state(w).workspace.layout.collapsed_column_for_group(group).unwrap();
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetColumnDrawers { column, drawers: true },
    });
}

#[test]
#[ignore = "private display native_numeric_preedit_guard"]
fn native_numeric_preedit_guard() {
    gtk::init().unwrap();
    let app = native_test_app("art.capycanvas.NumericPreedit");
    for theme in [Theme::Light, Theme::Dark] {
        adw::StyleManager::default().set_color_scheme(if theme == Theme::Dark { adw::ColorScheme::ForceDark } else { adw::ColorScheme::ForceLight });
        let control = crate::number_control::NumberControl::new(NumericControl::brush_size(), "Size", "", layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
        control.set_value(32.);
        let window = adw::ApplicationWindow::builder().application(&*app).content(&control).build();
        window.present();
        pump(60);
        let entry = descendant::<gtk::Entry>(&control).unwrap();
        let text = entry.delegate().and_downcast::<gtk::Text>().unwrap();
        let stack = descendant::<gtk::Stack>(&control).unwrap();
        let track = control.last_child().unwrap();
        let steps = [track.first_child().and_downcast::<gtk::Button>().unwrap(), track.last_child().and_downcast::<gtk::Button>().unwrap()];
        let scale = descendant::<gtk::Scale>(&control).unwrap();
        let display = stack.child_by_name("value").and_downcast::<gtk::Button>().unwrap();
        let display_controllers = display.observe_controllers();
        let wheel = (0..display_controllers.n_items()).find_map(|i| display_controllers.item(i).and_downcast::<gtk::EventControllerScroll>()).unwrap();
        stack.set_visible_child_name("entry");
        entry.grab_focus();
        entry.set_text("１２＋３ ｐｘ");
        let controllers = entry.observe_controllers();
        let keys = (0..controllers.n_items()).find_map(|i| controllers.item(i).and_downcast::<gtk::EventControllerKey>().filter(|controller| controller.name().as_deref() == Some("numeric-editor-cancel"))).unwrap();
        for preedit in ["にほんご", "简体", "繁體", "한국어", "１２"] {
            text.emit_preedit_changed(preedit);
            for key in [gdk::Key::Escape, gdk::Key::Return, gdk::Key::Down] {
                assert!(!keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &gdk::ModifierType::empty()]));
            }
            entry.emit_by_name::<()>("activate", &[]);
            for button in &steps { button.emit_clicked(); }
            scale.set_value(0.7);
            assert!(wheel.emit_by_name::<bool>("scroll", &[&0f64, &1f64]));
            assert_eq!(entry.text(), "１２＋３ ｐｘ");
            assert_eq!(stack.visible_child_name().as_deref(), Some("entry"));
            assert_eq!(control.value(), 32.);
        }
        text.emit_preedit_changed("");
        assert!(!control.commit_text());
        assert_eq!(control.value(), 32.);
        assert_eq!(entry.text(), "１２＋３ ｐｘ");
        let capture = (0..controllers.n_items()).find_map(|i| controllers.item(i).and_downcast::<gtk::EventControllerKey>().filter(|controller| controller.name().as_deref() == Some("numeric-composition-capture"))).unwrap();
        let changed = std::rc::Rc::new(std::cell::Cell::new(0));
        control.connect_value_changed(glib::clone!(#[strong] changed, move |_| changed.set(changed.get() + 1)));
        assert!(!control.input_valid());
        text.emit_preedit_changed("한국어");
        assert!(!capture.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
        text.emit_preedit_changed("");
        entry.set_text("12+3 px");
        assert!(!control.input_valid());
        for _ in 0..2 {
            assert!(!capture.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
            entry.emit_by_name::<()>("activate", &[]);
            assert!(keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
            assert_eq!(entry.text(), "12+3 px");
            assert_eq!(stack.visible_child_name().as_deref(), Some("entry"));
            assert_eq!(control.value(), 32.);
            assert_eq!(changed.get(), 0);
        }
        capture.emit_by_name::<()>("key-released", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]);
        entry.emit_by_name::<()>("activate", &[]);
        assert_eq!(control.value(), 32.);
        capture.emit_by_name::<()>("key-released", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]);
        assert!(control.input_valid());
        assert!(control.tooltip_text().is_none());
        assert_eq!(control.value(), 32.);
        assert_eq!(changed.get(), 0);
        assert!(!capture.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
        entry.emit_by_name::<()>("activate", &[]);
        assert_eq!(changed.get(), 1);
        capture.emit_by_name::<()>("key-released", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]);
        assert_eq!(control.value(), 15.);
        assert_eq!(stack.visible_child_name().as_deref(), Some("value"));
        for (literal, reason) in [("一二", layer_ui::NumericError::InvalidExpression), ("１２＋", layer_ui::NumericError::InvalidExpression), ("２３", layer_ui::NumericError::InvalidExpression), ("２＋３", layer_ui::NumericError::InvalidExpression), ("1/0", layer_ui::NumericError::FiniteNumber)] {
            stack.set_visible_child_name("entry");
            entry.set_text(literal);
            for button in &steps { button.emit_clicked(); }
            scale.set_value(0.7);
            assert!(wheel.emit_by_name::<bool>("scroll", &[&0f64, &1f64]));
            assert_eq!(entry.text(), literal);
            assert_eq!(control.value(), 15.);
            assert_eq!(stack.visible_child_name().as_deref(), Some("entry"));
            assert_eq!(control.tooltip_text().as_deref(), Some(reason.message(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).as_str()));
        }
        entry.set_text("16");
        steps[1].emit_clicked();
        assert_eq!(control.value(), 17.);
        stack.set_visible_child_name("entry");
        entry.set_text("４８");
        let refusal = control.tooltip_text();
        text.emit_preedit_changed("にほんご");
        assert!(!capture.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]));
        text.emit_preedit_changed("");
        assert!(keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]));
        assert_eq!(entry.text(), "４８");
        assert_eq!(control.value(), 17.);
        assert_eq!(control.tooltip_text(), refusal);
        assert_eq!(stack.visible_child_name().as_deref(), Some("entry"));
        capture.emit_by_name::<()>("key-released", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]);
        assert!(!capture.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]));
        assert!(keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]));
        capture.emit_by_name::<()>("key-released", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]);
        assert_eq!(control.value(), 17.);
        assert_eq!(stack.visible_child_name().as_deref(), Some("value"));
        let spin_control = crate::number_control::NumberControl::new(NumericControl::number(0., 16., 1., 0), "Count", "", layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
        spin_control.set_value(4.);
        let spin = descendant::<gtk::SpinButton>(&spin_control).unwrap();
        let spin_window = adw::ApplicationWindow::builder().application(&*app).content(&spin_control).build();
        spin_window.present();
        pump(40);
        for (literal, reason) in [("一二", layer_ui::NumericError::InvalidExpression), ("１２＋", layer_ui::NumericError::InvalidExpression), ("２３", layer_ui::NumericError::InvalidExpression), ("２＋３", layer_ui::NumericError::InvalidExpression), ("1/0", layer_ui::NumericError::FiniteNumber)] {
            spin.set_text(literal);
            assert!(!spin_control.commit_text());
            spin.update();
            for step in [gtk::ScrollType::StepUp, gtk::ScrollType::StepDown] { spin.emit_change_value(step); }
            assert_eq!(spin.value(), 4.);
            assert_eq!(spin.text(), literal);
            assert_eq!(spin_control.value(), 4.);
            assert_eq!(spin_control.tooltip_text().as_deref(), Some(reason.message(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).as_str()));
        }
        let spin_controllers = spin.observe_controllers();
        let spin_keys = (0..spin_controllers.n_items()).find_map(|i| spin_controllers.item(i).and_downcast::<gtk::EventControllerKey>().filter(|controller| controller.name().as_deref() == Some("numeric-editor-cancel"))).unwrap();
        assert!(spin_keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Escape, &0u32, &gdk::ModifierType::empty()]));
        assert_eq!(spin_control.value(), 4.);
        assert_eq!(spin.text(), "4");
        spin.set_text("１２＋３");
        let spin_text = spin.delegate().and_downcast::<gtk::Text>().unwrap();
        spin_text.emit_preedit_changed("にほんご");
        assert!(!spin_keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Escape, &0u32, &gdk::ModifierType::empty()]));
        assert!(!spin_control.commit_text());
        spin.update();
        spin_text.emit_by_name::<()>("activate", &[]);
        spin.emit_change_value(gtk::ScrollType::StepDown);
        assert_eq!(spin.text(), "１２＋３");
        assert_eq!(spin_control.value(), 4.);
        spin_text.emit_preedit_changed("");
        assert!(!spin_control.commit_text());
        assert_eq!(spin_control.value(), 4.);
        assert_eq!(spin.text(), "１２＋３");
        assert!(!spin_control.input_valid());
        let spin_capture = (0..spin_controllers.n_items()).find_map(|i| spin_controllers.item(i).and_downcast::<gtk::EventControllerKey>().filter(|controller| controller.name().as_deref() == Some("numeric-composition-capture"))).unwrap();
        spin_text.emit_preedit_changed("简体");
        assert!(!spin_capture.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
        spin_text.emit_preedit_changed("");
        spin.set_text("12+3");
        assert!(!spin_control.input_valid());
        assert!(!spin_control.commit_text());
        spin.update();
        spin_text.emit_by_name::<()>("activate", &[]);
        spin.emit_change_value(gtk::ScrollType::StepDown);
        assert_eq!(spin.text(), "12+3");
        assert_eq!(spin_control.value(), 4.);
        spin_capture.emit_by_name::<()>("key-released", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]);
        assert!(spin_control.input_valid());
        assert!(spin_control.tooltip_text().is_none());
        assert_eq!(spin_control.value(), 4.);
        assert!(!spin_capture.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
        assert!(spin_control.commit_text());
        assert_eq!(spin_control.value(), 15.);
        spin_capture.emit_by_name::<()>("key-released", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]);
        spin.set_text("２０００");
        assert!(!spin_control.commit_text());
        assert_eq!(spin_control.value(), 15.);
        spin.set_text("2000");
        assert!(spin_control.commit_text());
        assert_eq!(spin_control.value(), 16.);
        spin_window.close();
        window.close();
        pump(40);
    }
}

#[test]
#[ignore = "private display native_text_language"]
fn native_text_language() {
    {
        let (app, _) = crate::application("art.capycanvas.TextLanguageCold");
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let label = gtk::Label::new(Some("Capy Canvas"));
        let window = adw::ApplicationWindow::builder().application(&app).content(&label).build();
        window.present();
        pump(30);
        assert_eq!(label.layout().context().language(), Some(gtk::pango::Language::from_string(crate::launch_localization().language().tag())));
        window.close();
        pump(10);
        app.quit();
    }
    gtk::init().unwrap();
    for (index, language) in layer_ui::UiLanguage::ALL.into_iter().enumerate() {
        let app = adw::Application::builder().application_id(format!("art.capycanvas.TextLanguage{index}")).build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let localization = layer_ui::Localizer::shared(language);
        crate::text_language::install(&app, &localization);
        let expected = gtk::pango::Language::from_string(language.tag());
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let label = gtk::Label::new(Some(&format!("{} 日本語 简体 繁體 한국어 Русский Türkçe Tiếng Việt ไทย", localization.text(MessageId::COMMON_DRAWING_CANVAS))));
        label.set_wrap(true);
        label.set_max_width_chars(44);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_margin_top(8);
        label.set_margin_bottom(8);
        let entry = gtk::Entry::new();
        entry.set_width_chars(6);
        content.append(&label);
        content.append(&entry);
        let thai = (language == UiLanguage::Thai).then(|| {
            let paragraph = gtk::Label::new(Some("การเปลี่ยนภาษาไม่ควรเปลี่ยนภาพวาดหรือข้อความที่กำลังพิมพ์"));
            paragraph.set_wrap(true);
            paragraph.set_max_width_chars(20);
            paragraph.set_margin_start(8);
            paragraph.set_margin_end(8);
            content.append(&paragraph);
            paragraph
        });
        let window = adw::ApplicationWindow::builder().application(&app).content(&content).default_width(if language == UiLanguage::Thai { 200 } else { 480 }).default_height(300).build();
        for theme in [adw::ColorScheme::ForceLight, adw::ColorScheme::ForceDark] {
            adw::StyleManager::default().set_color_scheme(theme);
            window.present();
            pump(60);
            assert_eq!(label.layout().context().language(), Some(expected.clone()));
            assert_eq!(label.layout().unknown_glyphs_count(), 0);
            if let Some(paragraph) = &thai {
                assert_eq!(paragraph.layout().context().language(), Some(expected.clone()));
                eprintln!("GTK Thai diagnostics window {} × {}, paragraph {} × {}, measure {:?}, layout width {}, pixels {:?}, wrap {:?}", window.width(), window.height(), paragraph.width(), paragraph.height(), paragraph.measure(gtk::Orientation::Horizontal, -1), paragraph.layout().width(), paragraph.layout().pixel_size(), paragraph.wrap_mode());
                if paragraph.layout().line_count() <= 1 && let Ok(directory) = std::env::var("LAYER_TEST_ARTIFACTS") {
                    crate::snapshot_window(&window, 1.).save_to_png(std::path::PathBuf::from(directory).join("thai-failed-native-allocation.png")).unwrap();
                }
                assert!(paragraph.layout().line_count() > 1);
                assert_eq!(paragraph.layout().unknown_glyphs_count(), 0);
                eprintln!("GTK Thai paragraph actual allocation {} × {}, lines {}", paragraph.width(), paragraph.height(), paragraph.layout().line_count());
            }
            eprintln!("GTK font allocation {} {theme:?}: window {} × {}, label {} × {}, lines {}, layout width {}, pixels {:?}, measure {:?}", language.tag(), window.width(), window.height(), label.width(), label.height(), label.layout().line_count(), label.layout().width(), label.layout().pixel_size(), label.measure(gtk::Orientation::Horizontal, -1));
            if let Ok(directory) = std::env::var("LAYER_TEST_ARTIFACTS") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                crate::snapshot_window(&window, 1.).save_to_png(directory.join(format!("fonts-{}-{theme:?}.png", language.tag()))).unwrap();
            }
            assert_eq!(entry.pango_context().language(), Some(expected.clone()));
            let text = entry.delegate().and_downcast::<gtk::Text>().unwrap();
            assert_eq!(text.pango_context().language(), Some(expected.clone()));
            let added = gtk::Label::new(Some("漢字 한글"));
            content.append(&added);
            pump(20);
            assert_eq!(added.layout().context().language(), Some(expected.clone()));
            added.add_css_class("title-1");
            pump(20);
            assert_eq!(added.layout().context().language(), Some(expected.clone()));
            let other = adw::Window::builder().transient_for(&window).build();
            content.remove(&added);
            other.set_content(Some(&added));
            other.present();
            pump(20);
            assert_eq!(added.layout().context().language(), Some(expected.clone()));
            other.close();
        }
        window.close();
        pump(20);
        app.quit();
    }
}

#[test]
#[ignore = "private display and hardware GPU numeric size Apply"]
fn native_numeric_size_apply_refuses_uncommitted_text() {
    let app = native_test_app("art.capycanvas.NumericSizeApply");
    for theme in [Theme::Light, Theme::Dark] {
        let w = Workspace::new(&app);
        w.window.present();
        until(|| w.gpu.borrow().is_some() && w.workspaces.ready(), "numeric Apply workspace ready");
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for image in [false, true] {
            w.dispatch(UiAction::Invoke { command: if image { CommandId::ImageSize } else { CommandId::CanvasSize } });
            pump(60);
            let dialog = if image { &w.image_size.dialog } else { &w.canvas_size.dialog };
            let field = find_named(dialog.upcast_ref(), if image { "image-size-width" } else { "canvas-size-width" }).unwrap();
            let number = descendant::<crate::number_control::NumberControl>(&field).unwrap();
            let spin = descendant::<gtk::SpinButton>(&number).unwrap();
            let before = ui_session(&w).engine().document().clone();
            let old_value = number.value();
            for literal in ["一二", "１２＋", "１２＋３", "２＋３", "２３", "1/0"] {
                spin.set_text(literal);
                assert!(!number.commit_text());
                assert!(!dialog.is_response_enabled("apply"));
                assert!(!find_button(dialog.upcast_ref(), "Apply").unwrap().is_sensitive());
                w.window.grab_focus();
                pump(30);
                assert_eq!(spin.text(), literal);
                assert_eq!(number.value(), old_value);
                let current = ui_session(&w).engine().document().clone();
                assert_eq!((current.composition().size, current.revision), (before.composition().size, before.revision));
                assert!(if image { state(&w).layer_tools.image_size.is_some() } else { state(&w).layer_tools.canvas_size.is_some() });
            }
            spin.set_text(if image { "256" } else { "512" });
            assert!(number.commit_text());
            pump(30);
            assert!(dialog.is_response_enabled("apply"));
            find_button(dialog.upcast_ref(), "Apply").unwrap().emit_clicked();
            pump(180);
            assert_eq!(ui_session(&w).engine().document().composition().size[0], if image { 256 } else { 512 });
            assert!(if image { state(&w).layer_tools.image_size.is_none() } else { state(&w).layer_tools.canvas_size.is_none() });
        }
        w.window.close();
        pump(60);
    }
}


#[test]
#[ignore = "private display and hardware GPU localized production nested menus"]
fn native_localized_nested_menus() {
    let app = native_test_app("art.capycanvas.LocalizedNestedMenus");
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    new_photo::ready(&w);
    let mut workspace = state(&w).workspace;
    let first = workspace.layout.header.zones[HeaderZone::Left.index()].first().unwrap().id;
    workspace.layout.header.add(HeaderZone::Left, Some(first), &[HeaderItem::Menu]).unwrap();
    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
    let menu = named::<gtk::MenuButton>(w.window.upcast_ref(), "application-menu-Primary");
    let popup = menu.popover().unwrap().downcast::<gtk::PopoverMenu>().unwrap();
    let mut input = RemoteInput::new();
    input.ready();
    input.perform(serde_json::json!([{"wait_ms":200}]));
    let button = |parent: &gtk::PopoverMenu, label: &str| widgets(parent.upcast_ref()).find(|widget| widget.is_mapped() && widget.type_().name() == "GtkModelButton" && descendant::<gtk::Label>(widget).is_some_and(|text| text.label() == label)).unwrap();
    let open = |parent: &gtk::PopoverMenu, label: &str, input: &mut RemoteInput| {
        until(|| widgets(parent.upcast_ref()).any(|widget| widget.is_mapped() && widget.width() > 0 && widget.height() > 0 && widget.type_().name() == "GtkModelButton" && descendant::<gtk::Label>(&widget).is_some_and(|text| text.label() == label)), "localized submenu button allocated");
        let button = button(parent, label);
        let nested = button.property::<Option<gtk::PopoverMenu>>("popover").unwrap();
        let point = screen_point(&button, &w.window, [0.5, 0.5]);
        input.perform(serde_json::json!([{"point":point},{"wait_ms":200}]));
        if !nested.is_mapped() { input.click(point); }
        until(|| nested.is_mapped() && nested.width() > 0 && widgets(nested.upcast_ref()).any(|widget| widget.type_().name() == "GtkModelButton" && widget.width() > 0 && widget.height() > 0), "localized production submenu mapped and allocated");
        nested
    };
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32;
            w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(choice) } });
            until(|| w.localization().language() == language, "localized nested-menu language");
            if language == UiLanguage::Turkish {
                assert_eq!(ApplicationMenu::Edit.localized_label(&w.localization()), w.localization().text(MessageId::RESOURCES_LAYER_MENU_ORGANIZE), "real Turkish Edit and Organize intentionally share their visible label");
            }
            menu.popup();
            until(|| popup.is_mapped(), "localized production Primary menu mapped");
            let edit = open(&popup, &ApplicationMenu::Edit.localized_label(&w.localization()), &mut input);
            if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
                input.perform(serde_json::json!([{"wait_ms":200},{"capture":format!("edit-{}-{}", language.tag().to_lowercase(), format!("{theme:?}").to_lowercase())}]));
            }
            input.click(screen_point(&button(&edit, &CommandId::SearchCommands.localized_label(&w.localization())), &w.window, [0.5, 0.5]));
            until(|| state(&w).command_search.is_some(), "native localized Edit command action");
            w.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Close });
            menu.popup();
            until(|| popup.is_mapped(), "localized Layer menu parent mapped");
            let layer = open(&popup, &ApplicationMenu::Layer.localized_label(&w.localization()), &mut input);
            let organize = open(&layer, &w.localization().text(MessageId::RESOURCES_LAYER_MENU_ORGANIZE), &mut input);
            assert_ne!(edit, organize, "localized matching labels retain native submenu identities");
            if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
                input.perform(serde_json::json!([{"wait_ms":200},{"capture":format!("organize-{}-{}", language.tag().to_lowercase(), format!("{theme:?}").to_lowercase())}]));
            }
            let count = ui_session(&w).engine().document().scene().order().len();
            let checkpoint = ui_session(&w).engine().checkpoint();
            input.click(screen_point(&button(&organize, &w.localization().text(MessageId::RESOURCES_LAYER_MENU_DUPLICATE)), &w.window, [0.5, 0.5]));
            until(|| ui_session(&w).engine().document().scene().order().len() == count + 1, "native localized Organize Duplicate action");
            new_photo::ready(&w);
            w.dispatch(UiAction::Invoke { command: CommandId::Undo });
            new_photo::ready(&w);
            assert_eq!(ui_session(&w).engine().document().scene().order().len(), count);
            assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
        }
    }
    w.window.close();
    pump(60);
}

#[test]
#[ignore = "private display and hardware GPU live language switching"]
fn native_preferences_text_menu_live_language() {
    let (application, active) = crate::application("art.capycanvas.PreferencesTextMenuLanguages");
    let app = NativeTestApp(application);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    app.activate_action("new-window", None);
    until(|| !active.borrow().is_empty(), "preferences text-menu window");
    let w = active.borrow().last().unwrap().clone();
    new_photo::ready(&w);
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::OpenSettings { page: SettingsPage::Input });
        until(|| w.preferences.dialog.is_mapped(), "preferences native input page");
        let number = named::<crate::number_control::NumberControl>(w.preferences.dialog.upcast_ref(), "setting-prediction-horizon");
        let entry = descendant::<gtk::Entry>(&number).unwrap();
        let stack = descendant::<gtk::Stack>(&number).unwrap();
        stack.set_visible_child_name("entry");
        entry.grab_focus();
        pump(120);
        entry.set_text("tie\u{302}\u{301}ng ไทย 日本語");
        let popup = named::<gtk::PopoverMenu>(w.preferences.dialog.upcast_ref(), "preference-context-menu");
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32;
            w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(choice) } });
            until(|| w.localization().language() == language, "preferences text-menu current language");
            entry.grab_focus();
            entry.select_region(1, 5);
            let selection = entry.selection_bounds();
            let draft = entry.text();
            crate::preferences::show_reset_menu(&w, number.upcast_ref(), PreferenceId::PredictionHorizon, 1., 1.);
            until(|| popup.is_mapped(), "native preferences text menu mapped");
            let model = popup.menu_model().unwrap();
            let editing = model.item_link(0, "section").unwrap();
            let expected = layer_ui::text_edit_menu_localized(layer_ui::Platform::Gtk, &w.localization());
            assert_eq!(editing.n_items(), expected.len() as i32);
            for (index, item) in expected.iter().enumerate() {
                assert_eq!(editing.item_attribute_value(index as i32, "label", None).unwrap().get::<String>().unwrap().as_str(), item.label.as_ref());
            }
            assert_eq!(entry.text(), draft);
            assert_eq!(entry.selection_bounds(), selection);
            let other = layer_ui::localization::SHIPPED_LANGUAGES.iter().copied().find(|candidate| *candidate != language).unwrap();
            let next = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == other).unwrap() as u32;
            w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(next) } });
            pump(50);
            assert_eq!(w.localization().language(), language, "native menu owns publication boundary");
            assert_eq!(popup.menu_model().unwrap(), model);
            popup.activate_action("field.select-all", None).unwrap();
            assert_eq!(entry.selection_bounds(), Some((0, draft.chars().count() as i32)));
            popup.popdown();
            until(|| w.localization().language() == other, "preferences language publishes after menu closes");
            assert_eq!(named::<gtk::PopoverMenu>(w.preferences.dialog.upcast_ref(), "preference-context-menu"), popup);
            assert_eq!(descendant::<gtk::Entry>(&number).unwrap(), entry);
            assert_eq!(entry.text(), draft);
        }
        number.cancel_edit();
        w.dispatch(UiAction::CloseSettings);
        pump(60);
    }
    w.window.close();
    pump(60);
}

#[test]
#[ignore = "private display and hardware GPU live language switching"]
fn native_live_language_switching() {
    unsafe { std::env::set_var("GTK_A11Y", "test"); }
    let artifacts = std::env::var("LAYER_TEST_ARTIFACTS").ok().map(std::path::PathBuf::from);
    if let Some(path) = &artifacts { std::fs::create_dir_all(path).unwrap(); }
    let input = std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").map(|_| RefCell::new(RemoteInput::new().settle_ms(0).timeout_secs(30)));
    if let Some(input) = &input { input.borrow().ready(); }
    let choice_index = |language| 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32;
    let (application, active) = crate::application("art.capycanvas.LiveLanguageSwitching");
    let app = NativeTestApp(application);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    app.activate_action("new-window", None);
    until(|| !active.borrow().is_empty(), "new language window created");
    let w = active.borrow().last().unwrap().clone();
    new_photo::ready(&w);
    until(|| find_named(w.window.upcast_ref(), "application-menu-File").is_some(), "language workspace menu ready");
    let mut workspace = state(&w).workspace;
    let first = workspace.layout.header.zones[HeaderZone::Left.index()].first().unwrap().id;
    workspace.layout.header.add(HeaderZone::Left, Some(first), &[HeaderItem::Menu]).unwrap();
    let literal_title = "Tool 🎨 {literal}";
    let literal_toolbar = workspace.layout.add_toolbar(Some(workspace.layout.panel_group(Panel::Sizes).unwrap()), literal_title, &[]).unwrap();
    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
    until(|| find_named(w.window.upcast_ref(), "application-menu-Primary").is_some_and(|menu| menu.is_mapped()), "mapped primary language menu ready");
    let primary_menu = named::<gtk::MenuButton>(w.window.upcast_ref(), "application-menu-Primary");
    let group = state(&w).workspace.layout.panel_group(Panel::Sizes).unwrap();
    w.dispatch(UiAction::DoubleClickPanelHandle { group, viewport: [w.surface.width() as f32, w.surface.height() as f32] });
    enable_individual_column_panels(&w, group);

    let assert_header_bounds = || {
        if let Some(selector) = find_named(w.header.root.upcast_ref(), "header-workspace-selector").filter(|widget| widget.is_mapped()) {
            let id = state(&w).workspace.layout.header.entries().find(|entry| entry.item == HeaderItem::Workspaces).unwrap().id;
            let geometry = w.header.geometry_for_test();
            let slot = geometry.items.iter().find(|item| item.id == id).unwrap().bounds;
            let actual = selector.compute_bounds(&w.header.root).unwrap();
            assert!(actual.x() >= slot.x - 1. && actual.x() + actual.width() <= slot.x + slot.width + 1., "localized workspace caption respects shared bounds: {actual:?}, {slot:?}");
            let label = descendant::<gtk::Label>(&selector).unwrap();
            assert_eq!(label.ellipsize(), gtk::pango::EllipsizeMode::End);
        }
    };
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::Customize { action: CustomizationAction::ToggleColumnDrawer { group, panel: Panel::Sizes } });
        until(|| find_named(w.window.upcast_ref(), "drawer-control-brush-size").is_some_and(|number| number.is_mapped()), "retained brush size drawer visible");
        let drawer_number = named::<crate::number_control::NumberControl>(w.window.upcast_ref(), "drawer-control-brush-size");
        let drawer_tabs = [Panel::ToolSettings, Panel::Sizes, literal_toolbar].map(|panel| {
            assert_eq!(state(&w).workspace.layout.panel(panel).unwrap().custom_name(), (panel == literal_toolbar).then_some(literal_title));
            let button = named::<gtk::Button>(w.window.upcast_ref(), &format!("column-drawer-tab-{panel:?}"));
            let label = button.child().unwrap().last_child().and_downcast::<gtk::Label>().unwrap();
            (panel, button, label)
        });

        assert!(ui_session(&w).engine().document().working.selection.is_none());
        w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
        new_photo::ready(&w);
        let canvas_selection = ui_session(&w).engine().document().working.selection.clone().unwrap();
        let canvas_bounds = canvas_selection.coverage_bounds();
        assert_eq!(canvas_bounds.min, layer_core::Point { x: 0., y: 0. });
        assert_eq!(canvas_bounds.max, layer_core::Point { x: ui_session(&w).engine().document().composition().size[0] as f32, y: ui_session(&w).engine().document().composition().size[1] as f32 });
        assert_eq!(canvas_selection.contours()[0].len(), 4);
        assert!(!canvas_selection.inverted);
        let history_availability = (ui_session(&w).engine().can_undo(), ui_session(&w).engine().can_redo());
        let checkpoint = ui_session(&w).engine().checkpoint();
        w.dispatch(UiAction::OpenSettings { page: SettingsPage::Appearance });
        pump(100);
        let choice = named::<adw::ComboRow>(w.preferences.dialog.upcast_ref(), "setting-language");
        let menus = ApplicationMenu::ALL.map(|id| (id, named::<gtk::MenuButton>(w.window.upcast_ref(), &format!("application-menu-{id:?}"))));
        let session = ui_session(&w).engine() as *const _ as usize;
        let document_revision = ui_session(&w).engine().document().revision;
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            let selected = choice_index(language);
            let requested = Instant::now();
            let previous_language = w.localization().language();
            choice.set_selected(selected);
            until(|| w.localization().language() == language, "language choice visible");
            eprintln!("GTK language choice {} {theme:?}: publication observed {:?}", language.tag(), requested.elapsed());
            assert_eq!(ui_session(&w).localization().language(), language);
            assert_eq!(w.size_number.imp().editor_title.borrow().as_str(), w.localization().text(MessageId::WORKSPACE_CONTROL_BRUSH_SIZE).as_ref());
            assert_eq!(w.opacity.imp().editor_title.borrow().as_str(), w.localization().text(MessageId::TOOL_SETTING_OPACITY).as_ref());
            assert_eq!(w.view_info.field.imp().editor_title.borrow().as_str(), w.localization().text(MessageId::MENU_ZOOM).as_ref());
            assert_eq!(w.view_info.root.tooltip_text().as_deref(), Some(w.localization().text(MessageId::MENU_ZOOM).as_ref()));
            assert_eq!(w.view_info.field.tooltip_text().as_deref(), Some(w.localization().text(MessageId::MENU_ZOOM).as_ref()));
            assert_eq!(drawer_number.imp().editor_title.borrow().as_str(), w.localization().text(MessageId::WORKSPACE_CONTROL_BRUSH_SIZE).as_ref());
            assert_eq!(named::<crate::number_control::NumberControl>(w.window.upcast_ref(), "drawer-control-brush-size"), drawer_number);
            for (panel, button, label) in &drawer_tabs {
                let title = state(&w).workspace.layout.panel(*panel).unwrap().title_localized(&w.localization());
                assert_eq!(label.text().as_str(), title, "retained drawer tab {panel:?} follows {} {theme:?}", language.tag());
                if previous_language != language { assert_eq!(button.tooltip_text().as_deref(), Some(title.as_str())); }
                if *panel == literal_toolbar { assert_eq!(label.text().as_str(), literal_title); }
                assert_eq!(named::<gtk::Button>(w.window.upcast_ref(), &format!("column-drawer-tab-{panel:?}")), *button);
                assert_eq!(button.child().unwrap().last_child().and_downcast::<gtk::Label>().unwrap(), *label);
                let expected = std::ffi::CString::new(title.as_str()).unwrap();
                let difference = unsafe { gtk::ffi::gtk_test_accessible_check_property(button.as_ptr().cast(), gtk::ffi::GTK_ACCESSIBLE_PROPERTY_LABEL, expected.as_ptr()) };
                let difference: Option<glib::GString> = unsafe { glib::translate::from_glib_full(difference) };
                assert_eq!(difference, None, "retained drawer tab {panel:?} accessible title follows {} {theme:?}", language.tag());
            }
            assert_eq!(w.preferences.dialog.title().as_str(), w.localization().text(MessageId::SETTINGS_TITLE).as_ref());
            assert_eq!(choice.title().as_str(), ui_session(&w).preferences().unwrap().pages.iter().flat_map(|page| &page.groups).flat_map(|group| &group.rows).find(|row| row.id == PreferenceId::Language).unwrap().title);
            assert_eq!(named::<adw::ComboRow>(w.preferences.dialog.upcast_ref(), "setting-language"), choice);
            for (id, menu) in &menus {
                assert_eq!(menu.label().as_deref(), Some(id.localized_label(&w.localization()).as_ref()));
                assert_eq!(named::<gtk::MenuButton>(w.window.upcast_ref(), &format!("application-menu-{id:?}")), *menu);
            }
            assert_eq!(ui_session(&w).engine() as *const _ as usize, session);
            assert_eq!(ui_session(&w).engine().document().revision, document_revision);
            assert_eq!(ui_session(&w).engine().document().working.selection.as_ref(), Some(&canvas_selection));
            assert_eq!(ui_session(&w).engine().display_selection().as_deref(), Some(&canvas_selection));
            assert_eq!(ui_session(&w).engine().document().working.selection.as_ref().unwrap().coverage_bounds(), canvas_bounds);
            assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
            assert_eq!((ui_session(&w).engine().can_undo(), ui_session(&w).engine().can_redo()), history_availability);
            let pango_language = choice.pango_context().language().unwrap().to_string();
            if previous_language == language {
                assert_eq!(layer_ui::resolve_launch_language(LanguagePreference::System, &[pango_language.as_str()]), language);
            } else {
                assert_eq!(pango_language, gtk::pango::Language::from_string(language.tag()).to_string());
            }
            assert_eq!(w.screen.button.tooltip_text().as_deref(), Some(w.localization().text(MessageId::NATIVE_SCREEN_DETAILS).as_ref()));
            assert_eq!(named::<gtk::CheckButton>(w.screen.popover().upcast_ref(), "screen-mark-clipped").label().as_deref(), Some(w.localization().text(MessageId::NATIVE_HIGHLIGHT_CLIPPED_COLORS).as_ref()));
            pump(40);
            assert_header_bounds();
            if let Some(path) = &artifacts {
                save_snapshot(&w, 80, || path.join(format!("preferences-{}-{theme:?}.png", language.tag())));
                if matches!(language, UiLanguage::German | UiLanguage::French | UiLanguage::Thai) {
                    let settings = gtk::Settings::default().unwrap();
                    let font = settings.gtk_font_name();
                    let size = w.window.default_size();
                    settings.set_property("gtk-font-name", "Sans 16");
                    w.window.set_default_size(744, 780);
                    save_snapshot(&w, 250, || path.join(format!("preferences-large-text-narrow-{}-{theme:?}.png", language.tag())));
                    assert_header_bounds();
                    assert_eq!(choice.title().as_str(), ui_session(&w).preferences().unwrap().pages.iter().flat_map(|page| &page.groups).flat_map(|group| &group.rows).find(|row| row.id == PreferenceId::Language).unwrap().title);
                    eprintln!("GTK layout {} {theme:?}: requested 744 × 780, actual {} × {}, Sans 16", language.tag(), w.window.width(), w.window.height());
                    settings.set_property("gtk-font-name", font);
                    w.window.set_default_size(size.0, size.1);
                    pump(100);
                }
            }
        }
        for &language in layer_ui::localization::SHIPPED_LANGUAGES { choice.set_selected(choice_index(language)); }
        choice.set_selected(choice_index(UiLanguage::English));
        until(|| w.localization().language() == UiLanguage::English, "latest rapid language choice visible");
        choice.set_selected(0);
        let languages = glib::language_names_with_category("LC_MESSAGES");
        let tags = languages.iter().map(|tag| tag.as_str()).collect::<Vec<_>>();
        let system_language = layer_ui::resolve_launch_language(LanguagePreference::System, &tags);
        until(|| w.localization().language() == system_language, "system language visible");
        w.dispatch(UiAction::Preferences { action: PreferenceAction::ToggleSearch { open: true } });
        let settings_search = named::<gtk::SearchEntry>(w.preferences.dialog.upcast_ref(), "settings-search");
        settings_search.set_text("language 日本語 draft");
        settings_search.select_region(1, 4);
        let selection = settings_search.selection_bounds();
        choice.set_selected(choice_index(UiLanguage::Japanese));
        until(|| w.localization().language() == UiLanguage::Japanese, "pending settings search language visible");
        assert_eq!(settings_search.text(), "language 日本語 draft");
        assert_eq!(settings_search.selection_bounds(), selection);
        assert_eq!(ui_session(&w).preferences().unwrap().query, "language 日本語 draft");
        w.dispatch(UiAction::CloseSettings);
        w.dispatch(UiAction::Invoke { command: CommandId::CanvasSize });
        pump(100);
        let field = find_named(w.canvas_size.dialog.upcast_ref(), "canvas-size-width").unwrap();
        let number = descendant::<crate::number_control::NumberControl>(&field).unwrap();
        let spin = descendant::<gtk::SpinButton>(&number).unwrap();
        spin.grab_focus();
        pump(350);
        spin.set_text("１２＋漢字 abc");
        spin.select_region(1, 5);
        let selection = spin.selection_bounds();
        let revision = ui_session(&w).engine().document().revision;
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(choice_index(language)) } });
            until(|| w.localization().language() == language, "dirty dialog language visible");
            assert_eq!(descendant::<gtk::SpinButton>(&number).unwrap(), spin);
            assert_eq!(spin.text(), "１２＋漢字 abc");
            assert_eq!(spin.selection_bounds(), selection);
            assert!(!number.input_valid());
            assert!(!w.canvas_size.dialog.is_response_enabled("apply"));
            assert_eq!(ui_session(&w).engine().document().revision, revision);
            assert_eq!(w.canvas_size.dialog.heading().as_deref(), Some(state(&w).layer_tools.canvas_size.as_ref().unwrap().title.as_ref()));
        }
        w.dispatch(UiAction::CanvasSize { action: CanvasSizeAction::Cancel });
        let switch = |language| {
            w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(choice_index(language)) } });
            until(|| w.localization().language() == language, "retained workflow language visible");
        };
        w.dispatch(UiAction::OpenSettings { page: SettingsPage::Shortcuts });
        pump(100);
        let shortcut_search = named::<gtk::SearchEntry>(w.preferences.dialog.upcast_ref(), "shortcuts-search");
        shortcut_search.set_text("literal 日本語 🖌");
        shortcut_search.select_region(1, 4);
        let shortcut_selection = shortcut_search.selection_bounds();
        switch(UiLanguage::Korean);
        assert_eq!(shortcut_search.text(), "literal 日本語 🖌");
        assert_eq!(ui_session(&w).preferences().unwrap().shortcut_query, "literal 日本語 🖌");
        w.dispatch(UiAction::Preferences { action: PreferenceAction::BeginShortcut { id: CommandId::Redo.shortcut_id() } });
        for pressed in [true, false] {
            w.interact(UiInput::Key { key: "z".into(), pressed, repeat: false, modifiers: Modifiers { command: true, ..Default::default() }, editing: false, divider: None });
        }
        pump(100);
        let recording = named::<adw::ActionRow>(w.window.upcast_ref(), "shortcut-recording");
        let confirm = named::<gtk::Button>(recording.upcast_ref(), "confirm-shortcut");
        let cancel = named::<gtk::Button>(recording.upcast_ref(), "cancel-shortcut");
        assert!(confirm.is_sensitive());
        confirm.grab_focus();
        let focus = confirm.root().and_then(|root| root.focus());
        let chord = ui_session(&w).preferences().unwrap().capture.unwrap().chord;
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            let view = ui_session(&w).preferences().unwrap();
            let capture = view.capture.unwrap();
            assert_eq!(capture.chord, chord);
            assert_eq!(capture.conflict.as_deref(), Some(CommandId::Undo.localized_label(&w.localization()).as_ref()));
            assert_eq!(named::<adw::ActionRow>(w.window.upcast_ref(), "shortcut-recording"), recording);
            assert_eq!(named::<gtk::Button>(recording.upcast_ref(), "confirm-shortcut"), confirm);
            assert_eq!(named::<gtk::Button>(recording.upcast_ref(), "cancel-shortcut"), cancel);
            assert_eq!(recording.title().as_str(), capture.shortcut);
            assert_eq!(recording.subtitle().as_deref(), Some(capture.notice.as_str()));
            assert_eq!(confirm.label().as_deref(), Some(w.localization().text(MessageId::NATIVE_SHORTCUTS_REASSIGN).as_ref()));
            assert_eq!(cancel.label().as_deref(), Some(w.localization().text(MessageId::COMMON_CANCEL).as_ref()));
            assert_eq!(confirm.root().and_then(|root| root.focus()), focus);
            assert_eq!(named::<gtk::SearchEntry>(w.preferences.dialog.upcast_ref(), "shortcuts-search"), shortcut_search);
            assert_eq!(shortcut_search.text(), "literal 日本語 🖌");
            assert_eq!(shortcut_search.selection_bounds(), shortcut_selection);
            assert_eq!(view.shortcut_query, "literal 日本語 🖌");
        }
        click(&cancel);
        w.dispatch(UiAction::Preferences { action: PreferenceAction::CloseShortcutEditor });
        w.dispatch(UiAction::Preferences { action: PreferenceAction::OpenActionPicker { trigger: "touch.tap.2".into() } });
        pump(100);
        let picker_search = named::<gtk::SearchEntry>(w.window.upcast_ref(), "action-picker-search");
        picker_search.set_text("Undo");
        switch(UiLanguage::Japanese);
        assert_eq!(picker_search.text(), "Undo");
        assert_eq!(ui_session(&w).preferences().unwrap().shortcut_page.picker.unwrap().query, "Undo");
        picker_search.grab_focus();
        picker_search.select_region(1, 3);
        let selection = picker_search.selection_bounds();
        let focus = picker_search.root().and_then(|root| root.focus());
        let action = named::<adw::ActionRow>(w.window.upcast_ref(), "action-command.Undo");
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            assert_eq!(named::<gtk::SearchEntry>(w.window.upcast_ref(), "action-picker-search"), picker_search);
            assert_eq!(picker_search.text(), "Undo");
            assert_eq!(picker_search.selection_bounds(), selection);
            assert_eq!(picker_search.root().and_then(|root| root.focus()), focus);
            assert_eq!(ui_session(&w).preferences().unwrap().shortcut_page.picker.unwrap().query, "Undo");
            assert_eq!(named::<adw::ActionRow>(w.window.upcast_ref(), "action-command.Undo"), action);
            assert_eq!(action.title().as_str(), CommandId::Undo.localized_label(&w.localization()).as_ref());
        }
        w.dispatch(UiAction::Preferences { action: PreferenceAction::CloseActionPicker });
        w.dispatch(UiAction::CloseSettings);
        w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
        let command_search = named::<gtk::SearchEntry>(w.window.upcast_ref(), "command-search");
        let pencil_id = format!("command.{}", serde_json::to_value(CommandId::Pencil).unwrap().as_str().unwrap());
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            for query in [CommandId::Pencil.localized_label(&w.localization()).to_string(), "Pencil".into()] {
                command_search.set_text(&query);
                until(|| state(&w).command_search.as_ref().is_some_and(|view| view.query == query), "native localized command query");
                assert!(state(&w).command_search.unwrap().results.iter().any(|item| item.id == pencil_id));
            }
            if language == UiLanguage::German {
                let image_id = format!("command.{}", serde_json::to_value(CommandId::ImageSize).unwrap().as_str().unwrap());
                for query in ["BILDGRÖSSE", "BILDGRÖẞE", "IMAGE SIZE"] {
                    command_search.set_text(query);
                    until(|| state(&w).command_search.as_ref().is_some_and(|view| view.query == query), "native German sharp-S and English alias query");
                    assert!(state(&w).command_search.unwrap().results.iter().any(|item| item.id == image_id), "native Image Size result for {query}");
                }
            }
            command_search.set_text("İı Tiếng Việt ไทย {draft} 🎨");
            command_search.select_region(1, 4);
            let selection = command_search.selection_bounds();
            switch(UiLanguage::English);
            switch(language);
            assert_eq!(command_search.text(), "İı Tiếng Việt ไทย {draft} 🎨");
            assert_eq!(command_search.selection_bounds(), selection);
            assert_eq!(named::<gtk::SearchEntry>(w.window.upcast_ref(), "command-search"), command_search);
        }
        w.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Close });
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            let menu = &primary_menu;
            assert!(menu.is_mapped());
            menu.popup();
            until(|| menu.popover().is_some_and(|popup| popup.is_mapped()), "open menu before language request");
            let before = w.localization().language();
            w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(choice_index(language)) } });
            pump(50);
            assert_eq!(w.localization().language(), before, "open native menu defers language publication");
            assert_eq!(named::<gtk::MenuButton>(w.window.upcast_ref(), "application-menu-Primary"), *menu);
            menu.popdown();
            until(|| w.localization().language() == language, "language publication after menu selection boundary");
            menu.popup();
            if let Some(path) = &artifacts {
                save_snapshot(&w, 80, || path.join(format!("menu-{}-{theme:?}.png", language.tag())));
            }
            if let Some(input) = &input {
                input.borrow_mut().perform(serde_json::json!([{"wait_ms":160},{"capture":format!("menu-{}-{}", language.tag().to_lowercase(), format!("{theme:?}").to_lowercase())}]));
            }
            menu.popdown();
        }
        w.dispatch(UiAction::Customize { action: CustomizationAction::NewToolbar { group: None } });
        until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "tool-picker" && dialog.is_mapped()), "customization picker visible");
        let picker = w.window.visible_dialog().unwrap();
        let name = named::<adw::EntryRow>(picker.upcast_ref(), "toolbar-name");
        let search = named::<gtk::SearchEntry>(picker.upcast_ref(), "tool-search");
        let confirm = named::<gtk::Button>(picker.upcast_ref(), "confirm-tools");
        name.set_text("Artist 日本語 draft");
        search.set_text("pencil");
        switch(UiLanguage::Japanese);
        assert_eq!(search.text(), "pencil");
        assert_eq!(ui_session(&w).tool_picker().unwrap().query, "pencil");
        search.grab_focus();
        pump(350);
        search.select_region(1, 4);
        let selection = search.selection_bounds();
        let control = ui_session(&w).tool_picker().unwrap().choices[0].control;
        let choice_name = format!("tool-choice-{}", serde_json::to_string(&control).unwrap());
        let choice = named::<gtk::CheckButton>(picker.upcast_ref(), &choice_name);
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            let view = ui_session(&w).tool_picker().unwrap();
            assert_eq!(picker.title().as_str(), view.title.as_ref());
            assert_eq!(confirm.label().as_deref(), Some(view.confirm_label.as_ref()));
            assert_eq!(name.text(), "Artist 日本語 draft");
            assert_eq!(search.text(), "pencil");
            assert_eq!(search.selection_bounds(), selection);
            assert_eq!(named::<gtk::CheckButton>(picker.upcast_ref(), &choice_name), choice);
        }
        w.dispatch(UiAction::Customize { action: CustomizationAction::CancelTools });
        w.dispatch(UiAction::Customize { action: CustomizationAction::ManageToolbars });
        until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "toolbar-manager" && dialog.is_mapped()), "customization manager visible");
        let manager = w.window.visible_dialog().unwrap();
        let list = named::<gtk::ListBox>(manager.upcast_ref(), "managed-toolbars");
        let row = list.row_at_index(0).unwrap().downcast::<adw::ActionRow>().unwrap();
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            let view = ui_session(&w).toolbar_manager().unwrap();
            assert_eq!(manager.title().as_str(), view.title.as_ref());
            assert_eq!(list.row_at_index(0).unwrap().downcast::<adw::ActionRow>().unwrap(), row);
            assert_eq!(row.title().as_str(), view.toolbars[0].title);
            assert_eq!(row.subtitle().as_deref(), Some(view.toolbars[0].subtitle.as_str()));
        }
        w.dispatch(UiAction::Customize { action: CustomizationAction::CloseToolbarManager });
        new_photo::invoke(&w, CommandId::NewDocument);
        let dialog = w.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
        let width = named::<adw::SpinRow>(dialog.upcast_ref(), "new-document-width");
        width.grab_focus();
        pump(350);
        width.set_text("0034");
        width.select_region(1, 3);
        assert_eq!(width.text(), "0034");
        let selection = width.selection_bounds();
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            assert_eq!(named::<adw::SpinRow>(dialog.upcast_ref(), "new-document-width"), width);
            assert_eq!(width.text(), "0034");
            assert_eq!(width.selection_bounds(), selection);
            assert_eq!(dialog.heading().as_deref(), Some(layer_ui::new_document_spec(&w.localization()).title.as_ref()));
        }
        new_photo::response(&w, "cancel");
        new_photo::finish(&w);
        new_photo::invoke(&w, CommandId::ExportDocument);
        let export = w.window.visible_dialog().unwrap();
        let size = named::<adw::ComboRow>(export.upcast_ref(), "export-size");
        size.set_selected(1);
        new_photo::export_page(&w, "size");
        let width = named::<crate::number_control::NumberControl>(export.upcast_ref(), "export-width");
        let entry = descendant::<gtk::Entry>(&width).unwrap();
        descendant::<gtk::Stack>(&width).unwrap().set_visible_child_name("entry");
        entry.grab_focus();
        pump(350);
        entry.set_text("２３＋draft");
        entry.select_region(1, 4);
        let selection = entry.selection_bounds();
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            assert_eq!(named::<crate::number_control::NumberControl>(export.upcast_ref(), "export-width"), width);
            assert_eq!(entry.text(), "２３＋draft");
            assert_eq!(entry.selection_bounds(), selection);
            assert_eq!(size.selected(), 1);
            assert_eq!(named::<adw::NavigationView>(export.upcast_ref(), "export-navigation").visible_page_tag().as_deref(), Some("size"));
        }
        entry.set_text("640");
        assert!(width.commit_text());
        new_photo::export_page(&w, "main");
        new_photo::export_page(&w, "presets");
        named::<adw::ButtonRow>(export.upcast_ref(), "export-preset-save").emit_by_name::<()>("activated", &[]);
        pump(100);
        let name_dialog = w.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
        let name = named::<adw::EntryRow>(name_dialog.upcast_ref(), "export-preset-name");
        name.grab_focus();
        pump(350);
        name.set_text("私のdraft");
        name.select_region(1, 3);
        let selection = name.selection_bounds();
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            assert_eq!(named::<adw::EntryRow>(name_dialog.upcast_ref(), "export-preset-name"), name);
            assert_eq!(name.text(), "私のdraft");
            assert_eq!(name.selection_bounds(), selection);
            assert_eq!(name_dialog.response_label("cancel").as_str(), w.localization().text(MessageId::COMMON_CANCEL).as_ref());
        }
        new_photo::response(&w, "cancel");
        new_photo::response(&w, "cancel");
        new_photo::finish(&w);
        assert_eq!(ui_session(&w).engine() as *const _ as usize, session);
        assert_eq!(ui_session(&w).engine().document().revision, document_revision);
        switch(UiLanguage::Japanese);
        let previous = active.borrow().last().map(|workspace| workspace.window.clone());
        app.activate_action("new-window", None);
        until(|| active.borrow().last().is_some_and(|workspace| previous.as_ref() != Some(&workspace.window)), "new window follows current language");
        let other = active.borrow().last().unwrap().clone();
        assert_eq!(other.localization().language(), UiLanguage::Japanese);
        until(|| other.gpu.borrow().is_some(), "new window initialized");
        assert_eq!(ui_session(&other).localization().language(), UiLanguage::Japanese);
        other.window.close();
        pump(100);
        assert_eq!(ui_session(&w).engine().document().working.selection.as_ref(), Some(&canvas_selection));
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        new_photo::ready(&w);
        assert!(ui_session(&w).engine().document().working.selection.is_none());
        w.dispatch(UiAction::Invoke { command: CommandId::Redo });
        new_photo::ready(&w);
        assert_eq!(ui_session(&w).engine().document().working.selection.as_ref(), Some(&canvas_selection));
        assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
        w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
        new_photo::ready(&w);
    }
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "private display and hardware GPU localized documents and windows"]
fn native_live_language_documents_light() { live_language_documents(Theme::Light); }

#[test]
#[ignore = "private display and hardware GPU localized documents and windows"]
fn native_live_language_documents_dark() { live_language_documents(Theme::Dark); }

#[allow(deprecated)]
fn live_language_documents(theme: Theme) {
    let output = std::env::var("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join(format!("capy-language-documents-{}", std::process::id())));
    std::fs::create_dir_all(&output).unwrap();
    let (application, active) = crate::application("art.capycanvas.LiveLanguageDocuments");
    let app = NativeTestApp(application);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    app.activate_action("new-window", None);
    until(|| !active.borrow().is_empty(), "language document window created");
    let w = active.borrow().last().unwrap().clone();
    new_photo::ready(&w);
    app.activate_action("new-window", None);
    until(|| active.borrow().len() == 2, "existing second language window");
    let existing = active.borrow().last().unwrap().clone();
    new_photo::ready(&existing);
    let existing_engine = ui_session(&existing).engine() as *const _ as usize;
    w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit {
                id: PreferenceId::Language,
                value: PreferenceValue::Choice(1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32),
            } });
            until(|| w.localization().language() == language && existing.localization().language() == language, "existing windows adopt language");
            assert_eq!(ui_session(&existing).engine() as *const _ as usize, existing_engine);
            let literal = format!("{} {theme:?} İı Tiếng Việt Tiếng Việt ไทย 日本語 🎨 {{draft}}", language.tag());
            let occurrence = ui_session(&w).engine().document().working.occurrence.unwrap();
            let occurrence_id = ui_session(&w).engine().document().artwork.occurrences.id(occurrence).unwrap();
            let layer = layer_ui::occurrence_token(occurrence);
            w.dispatch(UiAction::Layer { action: LayerAction::BeginRename { id: layer } });
            let row = named::<gtk::Widget>(w.window.upcast_ref(), &format!("art-layer-{layer}"));
            let entry = find_css(&row, "layer-name-entry").unwrap().downcast::<gtk::Entry>().unwrap();
            entry.set_text(&literal);
            entry.select_region(1, 5);
            let selection = entry.selection_bounds();
            w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(1) } });
            until(|| w.localization().language() == layer_ui::localization::SHIPPED_LANGUAGES[0], "switch back while rename draft retained");
            w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32) } });
            until(|| w.localization().language() == language, "return to language with rename draft");
            assert_eq!(entry.text(), literal);
            assert_eq!(entry.selection_bounds(), selection);
            assert_eq!(find_css(&row, "layer-name-entry").unwrap().downcast::<gtk::Entry>().unwrap(), entry);
            entry.emit_activate();
            until(|| ui_session(&w).engine().document().scene().occurrence(occurrence).unwrap().name.as_ref() == literal.as_str(), "Unicode layer name committed");
            w.dispatch(UiAction::Invoke { command: CommandId::Pen });
            w.dispatch(UiAction::SetBrushSize { value: 17. });
            w.dispatch(UiAction::SetColor { rgba: [0.12, 0.38, 0.72, 1.] });
            new_photo::ready(&w);
            let before = glib::MainContext::default().block_on(read_canvas_pixels(&w, 9910)).unwrap().bytes;
            let y = (if theme == Theme::Dark { 190. } else { 30. }) + 7. * layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as f32;
            native_pen_path(&w, &[[30., y], [60., y], [100., y]]);
            new_photo::ready(&w);
            let painted = glib::MainContext::default().block_on(read_canvas_pixels(&w, 9911)).unwrap().bytes;
            assert_ne!(painted, before, "continued drawing {} {theme:?}", language.tag());
            w.dispatch(UiAction::Invoke { command: CommandId::Undo });
            new_photo::ready(&w);
            assert_eq!(glib::MainContext::default().block_on(read_canvas_pixels(&w, 9912)).unwrap().bytes, before);
            w.dispatch(UiAction::Invoke { command: CommandId::Redo });
            new_photo::ready(&w);
            assert_eq!(glib::MainContext::default().block_on(read_canvas_pixels(&w, 9913)).unwrap().bytes, painted);
            let filename = format!("{}-{theme:?}-Tiếng Việt-ไทย-🎨.capy", language.tag());
            new_photo::invoke(&w, CommandId::SaveDocumentAs);
            let save = new_photo::chooser();
            save.set_current_folder(Some(&gtk::gio::File::for_path(&output))).unwrap();
            save.set_current_name(&filename);
            pump(100);
            save.response(gtk::ResponseType::Accept);
            new_photo::finish(&w);
            assert!(!state(&w).document_file.modified);
            let path = output.join(&filename);
            assert!(path.is_file());
            let original = w.documents.selected();
            let count = w.documents.len();
            new_photo::invoke(&w, CommandId::OpenDocument);
            let open = new_photo::chooser();
            open.set_file(&gtk::gio::File::for_path(&path)).unwrap();
            pump(100);
            open.response(gtk::ResponseType::Accept);
            new_photo::finish(&w);
            until(|| w.documents.len() == count + 1 && w.documents.selected() != original, "saved Unicode drawing reopened in new tab");
            new_photo::ready(&w);
            assert_eq!(ui_session(&w).localization().language(), language);
            let restored = ui_session(&w).engine().document().artwork.occurrences.resolve(occurrence_id).unwrap();
            assert_eq!(ui_session(&w).engine().document().scene().occurrence(restored).unwrap().name.as_ref(), literal.as_str());
            assert_eq!(glib::MainContext::default().block_on(read_canvas_pixels(&w, 9914)).unwrap().bytes, painted);
            let reopened = w.documents.selected();
            new_photo::invoke(&w, CommandId::ExportDocument);
            new_photo::response(&w, "export");
            let save = new_photo::chooser();
            let export_name = format!("{}-{theme:?}-Tiếng Việt-ไทย-🎨.png", language.tag());
            save.set_current_folder(Some(&gtk::gio::File::for_path(&output))).unwrap();
            save.set_current_name(&export_name);
            pump(100);
            save.response(gtk::ResponseType::Accept);
            new_photo::finish(&w);
            let image = layer_color::photo::read_photo(std::io::BufReader::new(std::fs::File::open(output.join(export_name)).unwrap()), Default::default()).unwrap();
            assert_eq!(image.extent, [ui_session(&w).engine().document().composition().size[0], ui_session(&w).engine().document().composition().size[1]]);
            w.documents.select(&w, reopened, true);
            until(|| w.documents.len() == count && w.documents.selected() == original, "return to existing drawing tab");
            new_photo::ready(&w);
            assert_eq!(ui_session(&w).localization().language(), language);
            assert_eq!(glib::MainContext::default().block_on(read_canvas_pixels(&w, 9915)).unwrap().bytes, painted);
            let windows = active.borrow().len();
            app.activate_action("new-window", None);
            until(|| active.borrow().len() == windows + 1, "future window created in active language");
            let future = active.borrow().last().unwrap().clone();
            new_photo::ready(&future);
            assert_eq!(future.localization().language(), language);
            assert_eq!(ui_session(&future).localization().language(), language);
            future.window.close();
            until(|| active.borrow().len() == windows, "owned future window closed");
            w.window.present();
            save_snapshot(&w, 100, || output.join(format!("drawing-{}-{theme:?}.png", language.tag())));
        }
    existing.window.close();
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "private IBus engine and native compositor input"]
fn native_genuine_language_composition() {
    assert!(std::env::var("WAYLAND_DISPLAY").unwrap().starts_with("layer-bench-"));
    assert!(crate::storage::workspaces().is_some() && crate::storage::sessions().is_some(), "private fixture requires workspace and session storage");
    let result = std::path::PathBuf::from(std::env::var_os("LAYER_IME_RESULT").unwrap());
    assert!(!result.exists());
    let (application, active) = crate::application("art.capycanvas.LocalizationResolutionIme");
    let app = NativeTestApp(application);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    if let Some(photo) = std::env::var_os("CAPY_IME_TEST_PHOTO") {
        app.open(&[gtk::gio::File::for_path(photo)], "");
    } else {
        app.activate();
    }
    let deadline = Instant::now() + Duration::from_secs(150);
    let mut numeric_snapshot = None;
    while !result.exists() {
        assert!(Instant::now() < deadline, "genuine native engine acceptance timed out");
        pump(20);
        if let Some(w) = active.borrow().last() {
            let dialog = &w.image_size.dialog;
            let snapshot = if dialog.is_mapped() {
                let field = find_named(dialog.upcast_ref(), "image-size-width").unwrap();
                let number = descendant::<crate::number_control::NumberControl>(&field).unwrap();
                let spin = descendant::<gtk::SpinButton>(&number).unwrap();
                let text = spin.delegate().and_downcast::<gtk::Text>().unwrap();
                let document = ui_session(w).engine().document().clone();
                serde_json::json!({"dialog_visible":true,"language":w.localization().language().tag(),"draft":spin.text().to_string(),"selection":spin.selection_bounds(),"value":number.value(),"input_valid":number.input_valid(),"error_reason":number.imp().error.borrow().clone(),"error_caption":number.tooltip_text().map(|caption|caption.to_string()),"composing":number.composing(),"editor_size":[text.width(),text.height()],"editor_identity":format!("{:p}",text.as_ptr()),"control_identity":format!("{:p}",number.as_ptr()),"focused":text.has_focus(),"point":screen_point(text.upcast_ref(),&w.window,[0.5,0.5]),"apply_enabled":dialog.is_response_enabled("apply"),"document":[document.composition().size[0],document.composition().size[1],document.revision]})
            } else { serde_json::json!({"dialog_visible":false}) };
            if numeric_snapshot.as_ref() != Some(&snapshot) {
                let state = result.with_file_name("numeric-state.json");
                let temporary = state.with_extension("tmp");
                std::fs::write(&temporary, serde_json::to_vec(&snapshot).unwrap()).unwrap();
                std::fs::rename(temporary, state).unwrap();
                numeric_snapshot = Some(snapshot);
            }
        }
    }
    let evidence: serde_json::Value = serde_json::from_slice(&std::fs::read(result).unwrap()).unwrap();
    assert_eq!(evidence["entry_identity_preserved"], true);
    drop(active);
    app.quit();
}
