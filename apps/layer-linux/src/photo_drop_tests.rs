//! File-manager-style URI delivery from another process through real Wayland DND.
//! Run in the isolated compositor; no callback injection into production targets.
use super::new_photo::{finish, invoke, ready};
use super::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[path = "photo_workflow_tests.rs"]
mod workflow;
pub(super) use workflow::frames;

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_photo_transform_pixels_workflow() {
    let app = native_test_app("art.capycanvas.PhotoTransformPixels");
    let theme = match std::env::var("CAPY_NATIVE_TEST_THEME").as_deref().unwrap_or("dark") {
        "light" => layer_ui::Theme::Light,
        "dark" => layer_ui::Theme::Dark,
        _ => panic!("CAPY_NATIVE_TEST_THEME must be light or dark"),
    };
    let mut project = new_drawing(200, 150, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let id = project.document.active_layer;
    let photo = project.document.layers.iter_mut().find(|layer| layer.id == id).unwrap();
    photo.source = Some(layer_core::color::source::rgba8_source([320, 240], |x, y| {
        if (x / 16 + y / 16) % 2 == 0 { [230, 40, 80, 255] } else { [20, 160, 220, 255] }
    }));
    photo.properties.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([0.5, 0., 0., 0.5, 20., 15.]));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    w.dispatch(UiAction::SetTheme { theme: Some(theme) });
    pump(100);
    assert_eq!(state(&w).theme, theme);
    let narrow=std::env::var("LAYER_MOTION_VIEWPORT").as_deref()==Ok("640x480");
    if narrow {
        for panel in layer_ui::Panel::ALL.into_iter().filter(|panel| !matches!(panel,layer_ui::Panel::Toolbar|layer_ui::Panel::Commands|layer_ui::Panel::ToolSettings)) {
            w.dispatch(UiAction::Customize {action:layer_ui::CustomizationAction::SetPanelVisible{panel,visible:false}});
        }
        ready(&w);
    }
    invoke(&w,CommandId::FitCanvas);ready(&w);
    let current = || ui_session(&w).engine().document().layer(id).unwrap().clone();
    let capture = |name: &str| {
        if let Some(path) = std::env::var_os("LAYER_IMAGE_CAPTURE_DIR") {
            let directory = std::path::PathBuf::from(path);
            std::fs::create_dir_all(&directory).unwrap();
            super::new_photo::capture_ui(&w, &directory, name);
        }
    };
    invoke(&w, CommandId::ScaleRotate);
    until(|| state(&w).commands.iter().any(|c| c.id == CommandId::PlacementOriginalSize && c.enabled), "photo transform preparation");
    w.dispatch(UiAction::SetToolSetting { id: "transform_width".into(), value: 0.6 });
    invoke(&w, CommandId::ApplyTransform);
    ready(&w);
    invoke(&w, CommandId::Brush);
    w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::WetWatercolor as u32 });
    w.dispatch(UiAction::SetBrushSize { value: 30. });
    w.dispatch(UiAction::SetColor { rgba: [0.15, 0.25, 0.9, 1.] });
    ready(&w);
    assert_eq!(ui_session(&w).engine().brush().wet_mix.wetness, 0.);
    native_pen_path(&w, &[[50., 65.], [75., 65.], [100., 65.]]);
    ready(&w);
    let raw = current();
    w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::AddMask {id:id.0,replace:false}});
    w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Select {id:id.0,mask:false}});
    invoke(&w,CommandId::ScaleRotate);
    until(|| state(&w).canvas_bar.is_some_and(|bar|matches!(bar.context.kind,layer_ui::CanvasBarKind::Transform|layer_ui::CanvasBarKind::Placement)),"retained Transform opens");
    invoke(&w,CommandId::TransformDistort);
    let mut native=super::canvas_bar_tests::remote_input();
    let corner=ui_session(&w).engine().document().layer_geometry(id).map(Point{x:320.,y:0.}).unwrap();
    let corner=super::canvas_bar_tests::canvas_point(&w,[corner.x,corner.y]);
    let area=w.area.compute_bounds(&w.window).unwrap();
    assert!(corner[0]>area.x()+12. && corner[0]<area.x()+area.width()-12.
        && corner[1]>area.y()+12. && corner[1]<area.y()+area.height()-12.,"Distort handle is visible: {corner:?} in {area:?}");
    native.perform(json!([{"point":corner},{"down":true},{"wait_ms":40},
        {"point":[corner[0]-25.,corner[1]+12.]},{"wait_ms":30},{"down":false}]));
    invoke(&w,CommandId::ApplyTransform);ready(&w);
    assert!(current().properties.placement.as_affine().is_none(),"Distort persists a homography");
    invoke(&w,CommandId::ScaleRotate);
    until(|| state(&w).canvas_bar.is_some_and(|bar|matches!(bar.context.kind,layer_ui::CanvasBarKind::Transform|layer_ui::CanvasBarKind::Placement)),"retained Distort reopens");
    invoke(&w,CommandId::TransformWarp);
    let preview=|| ui_session(&w).engine().document().layer_geometry(id);
    let map=preview();let mesh=layer_core::MeshMap::identity(layer_core::Rect::from_extent(current().local_extent([200,150])),layer_core::MeshMap::PRESETS[0]).unwrap();let cells=mesh.cells();
    let split=map.map(mesh.frame.map(Point {x:0.37,y:0.61})).unwrap();
    invoke(&w,CommandId::WarpSplitCross);
    native.click(super::canvas_bar_tests::canvas_point(&w,[split.x,split.y]));
    until(|| preview().placement.mesh.as_ref().is_some_and(|mesh|mesh.cells()==[cells[0]+1,cells[1]+1]),"Cross inserts two nonuniform grid lines");
    let node=|index| {let map=preview();let p=map.placement.outer.map(map.placement.mesh.as_ref().unwrap().node(index).unwrap()).unwrap();
        super::canvas_bar_tests::canvas_point(&w,[p.x,p.y])};
    let width=u32::from(preview().placement.mesh.as_ref().unwrap().cells()[0])+1;
    let indices=[width+1,width+2];
    invoke(&w,CommandId::WarpSelectPoints);
    for index in indices {native.click(node(index));}
    invoke(&w,CommandId::WarpSelectPoints);
    let before=indices.map(node);let from=before[0];
    native.perform(json!([{"point":from},{"down":true},{"wait_ms":40},
        {"point":[from[0]+18.,from[1]+12.]},{"wait_ms":30},{"down":false}]));
    until(|| indices.into_iter().zip(before).all(|(index,p)| {let now=node(index);(now[0]-p[0]).hypot(now[1]-p[1])>10.}),"selected Warp points move together");
    capture("retained-warp.png");
    invoke(&w,CommandId::ApplyTransform);ready(&w);
    assert!(current().properties.placement.mesh.is_some());
    assert_eq!(current().raster,raw.raster,"retained geometry keeps raw material immutable");
    assert_eq!(current().source,raw.source,"retained geometry keeps the original photo");
    let retained = current();
    let material = retained.raster.wait_data().unwrap();
    let planes = |data: &layer_core::raster::RasterData| data.tiles.keys().map(|key| key.plane)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(planes(&material), std::collections::BTreeSet::from([
        layer_core::raster::RasterPlane::Color, layer_core::raster::RasterPlane::WatercolorWetness]));
    assert!(material.watercolor.is_some());
    assert_ne!(retained.properties.placement, layer_core::LayerPlacement::IDENTITY);
    w.dispatch(UiAction::Invoke { command: CommandId::ApplyTransformPixels });
    assert!(state(&w).commands.iter().any(|c| c.id == CommandId::CancelTransform && c.enabled));
    w.dispatch(UiAction::Invoke { command: CommandId::CancelTransform });
    pump(300);
    assert_eq!(current(), retained, "cancelled worker cannot publish a bake");
    w.dispatch(UiAction::Invoke { command: CommandId::ApplyTransformPixels });
    capture("applying.png");
    until(|| current().source.is_none(), "bake worker publishes native pixels");
    ready(&w);
    let baked = current();
    assert_eq!(baked.properties.placement, layer_core::LayerPlacement::IDENTITY);
    assert!(!baked.raster.is_empty());
    let baked_material = baked.raster.wait_data().unwrap();
    assert_eq!(planes(&baked_material), planes(&material));
    assert_eq!(baked_material.watercolor, material.watercolor);
    invoke(&w, CommandId::Pen);
    w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::GPen as u32 });
    w.dispatch(UiAction::SetBrushSize { value: 15. });
    w.dispatch(UiAction::SetColor { rgba: [1., 0., 1., 1.] });
    native_pen_path(&w, &[[50., 65.], [75., 65.], [100., 65.]]);
    ready(&w);
    let painted = current();
    assert_ne!(painted.raster, baked.raster);
    capture("editing.png");
    if std::env::var_os("LAYER_IMAGE_CAPTURE_DIR").is_some()
        && narrow {
        assert!(w.window.is_maximized());
        assert_eq!((w.surface.width(), w.surface.height()), (640, 480));
        invoke(&w, CommandId::ScaleRotate);
        until(|| state(&w).canvas_bar.is_some_and(|bar| matches!(bar.context.kind,layer_ui::CanvasBarKind::Transform|layer_ui::CanvasBarKind::Placement))
            && super::canvas_bar_tests::shown(&w), "narrow transform bar is visible");
        let bounds = w.canvas_bar.root.compute_bounds(&w.window).unwrap();
        assert!(bounds.x() >= 0. && bounds.y() >= 0.
            && bounds.x() + bounds.width() <= 640. && bounds.y() + bounds.height() <= 480.,
            "narrow transform bar must fit the allocated window: {bounds:?}");
        capture("narrow-transform.png");
        let more = super::canvas_bar_tests::bar_widget(&w, "canvas-bar-more");
        let more_point=super::canvas_bar_tests::center(&w,&more);
        native.click(more_point);
        until(|| w.canvas_bar.menu_open(), "narrow More menu opens");
        capture("narrow-more.png");
        native.key(0xff1b);
        until(|| !w.canvas_bar.menu_open(), "Escape closes narrow More");
        let cancel = super::canvas_bar_tests::bar_widget(&w, "canvas-bar-CancelTransform");
        native.click(super::canvas_bar_tests::center(&w, &cancel));
        until(|| state(&w).layer_tools.tool != LayerCanvasTool::Transform, "narrow Cancel returns to editing");
        ready(&w);
        assert_eq!(current(), painted);
        capture("narrow-editing.png");
        w.window.maximize();
        pump(350);
        ready(&w);
    }
    invoke(&w, CommandId::Liquify);
    w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::LiquifyTwirl as u32 });
    w.dispatch(UiAction::SetBrushSize { value: 40. });
    native_pen_path(&w, &[[80., 70.], [95., 75.], [110., 80.]]);
    ready(&w);
    let liquified = current();
    assert_ne!(liquified.raster, painted.raster);
    invoke(&w, CommandId::Undo); ready(&w); assert_eq!(current(), painted);
    invoke(&w, CommandId::Undo); ready(&w); assert_eq!(current(), baked);
    invoke(&w, CommandId::Undo); ready(&w); assert_eq!(current(), retained);
    invoke(&w, CommandId::Redo); ready(&w); assert_eq!(current(), baked);
    let saved = super::place_source::snapshot(&w);
    let reopened = layer_core::Project::read(saved.as_slice(), Default::default()).unwrap();
    let restored_layer = reopened.document.layer(id).unwrap();
    assert_eq!(restored_layer.properties, baked.properties);
    assert!(restored_layer.source.is_none());
    let digests = |layer: &layer_core::Layer| layer.raster.wait_data().unwrap().tiles.iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().digest)).collect::<Vec<_>>();
    assert_eq!(digests(restored_layer), digests(&baked));
    assert_eq!(restored_layer.raster.wait_data().unwrap().watercolor, material.watercolor);
    let before = glib::MainContext::default().block_on(read_canvas_pixels(&w, 9981)).unwrap();
    w.window.destroy(); pump(100);
    let restored = Workspace::with_project(&app, Some((reopened, None)));
    restored.window.maximize(); restored.window.present(); ready(&restored);
    restored.dispatch(UiAction::SetTheme { theme: Some(theme) });
    pump(100);
    assert_eq!(state(&restored).theme, theme);
    let after = glib::MainContext::default().block_on(read_canvas_pixels(&restored, 9982)).unwrap();
    assert!(before.bytes == after.bytes, "baked artwork survives native reopen");
    restored.window.destroy(); pump(100);
}

fn publish(path: &Path, value: &Value) {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(value).unwrap()).unwrap();
    std::fs::rename(temporary, path).unwrap();
}
fn read(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// This helper is launched by the receiving test. It has its own GDK display
/// connection, and offers only text/uri-list, forcing native MIME negotiation.
#[test]
#[ignore = "child process of native_photo_file_drops"]
fn native_photo_file_drag_source() {
    assert_eq!(std::env::var("LAYER_PHOTO_DROP_HELPER").as_deref(), Ok("1"));
    let dir = PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.PhotoDragSource");
    let label = gtk::Label::new(Some("Drag image files from this window"));
    let window = gtk::ApplicationWindow::builder()
        .application(&*app)
        .title("Photo file drag source")
        .decorated(false)
        .child(&label)
        .build();
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::COPY);
    source.connect_prepare(glib::clone!(
        #[strong]
        dir,
        move |_, _, _| {
            let files = read(&dir.join("source-files.json"))?;
            let uris = files
                .as_array()?
                .iter()
                .map(|path| {
                    format!(
                        "{}\r\n",
                        gtk::gio::File::for_path(path.as_str().unwrap()).uri()
                    )
                })
                .collect::<String>();
            Some(gdk::ContentProvider::for_bytes(
                "text/uri-list",
                &glib::Bytes::from_owned(uris),
            ))
        }
    ));
    let begins = Rc::new(Cell::new(0));
    source.connect_drag_begin(glib::clone!(
        #[strong]
        begins,
        #[strong]
        dir,
        move |_, drag| {
            begins.set(begins.get() + 1);
            publish(
                &dir.join("source-begin.json"),
                &json!({
                    "count": begins.get(), "device": format!("{:?}", drag.device().source()),
                }),
            );
        }
    ));
    label.add_controller(source);
    window.maximize();
    window.present();
    let deadline = Instant::now() + Duration::from_secs(180);
    while !dir.join("source-stop").exists() {
        assert!(Instant::now() < deadline, "receiving test did not finish");
        pump(20);
        publish(
            &dir.join("source-window.json"),
            &json!({
                "width": window.width(), "height": window.height(), "active": window.is_active(),
            }),
        );
    }
    window.destroy();
}

struct FileDrag {
    input: RemoteInput,
    child: std::process::Child,
    begins: u64,
    start: [f32; 2],
}
impl Drop for FileDrag {
    fn drop(&mut self) {
        let _ = std::fs::write(self.input.dir.join("source-stop"), b"stop");
        // Reap this test's own child even during an assertion failure.
        pump(100);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl FileDrag {
    fn stop_source(&mut self) {
        std::fs::write(self.input.dir.join("source-stop"), b"stop").unwrap();
        until(|| self.child.try_wait().unwrap().is_some(), "source shutdown");
        pump(200);
    }
    fn start(w: &Workspace) -> Self {
        let input = RemoteInput::new().settle_ms(150).timeout_secs(15);
        input.ready();
        pump(500);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg(format!(
                "{}::native_photo_file_drag_source",
                module_path!().split_once("::").unwrap().1
            ))
            .args(["--exact", "--ignored", "--nocapture"])
            .env("LAYER_PHOTO_DROP_HELPER", "1")
            .spawn()
            .unwrap();
        let mut driver = Self {
            input,
            child,
            begins: 0,
            start: [0.; 2],
        };
        until(
            || {
                read(&driver.input.dir.join("source-window.json"))
                    .is_some_and(|v| v["active"] == true)
            },
            "source activation",
        );
        // Mutter's standard Super+Left tiles the source to the left half of the
        // private display. The maximized editor stays visible on the right.
        driver.input.perform(json!([
            {"key": 0xffeb, "down": true}, {"key": 0xff51, "down": true},
            {"key": 0xff51, "down": false}, {"key": 0xffeb, "down": false}
        ]));
        let width = w.window.width();
        until(
            || {
                read(&driver.input.dir.join("source-window.json"))
                    .is_some_and(|v| v["width"].as_i64() == Some(i64::from(width / 2)))
            },
            "source must tile left",
        );
        driver.start = [width as f32 * 0.25, w.window.height() as f32 * 0.5];
        driver
    }
    fn hover(&mut self, paths: &[PathBuf], point: [f32; 2], touch: bool) {
        publish(&self.input.dir.join("source-files.json"), &json!(paths));
        if read(&self.input.dir.join("source-window.json")).unwrap()["active"] != true {
            self.input.perform(json!([
                {"key": 0xffe9, "down": true}, {"key": 0xff09, "down": true},
                {"key": 0xff09, "down": false}, {"key": 0xffe9, "down": false}
            ]));
        }
        until(
            || {
                read(&self.input.dir.join("source-window.json"))
                    .is_some_and(|v| v["active"] == true)
            },
            "source focus",
        );
        let slop = gtk::Settings::default().unwrap().gtk_dnd_drag_threshold() as f32;
        let pickup = [self.start[0] + slop * 3., self.start[1]];
        let armed = [pickup[0] + slop * 3., pickup[1]];
        self.input.perform(if touch {
            json!([
                {"touch": "down", "point": self.start}, {"wait_ms": 80},
                {"touch": "move", "point": pickup}, {"wait_ms": 80},
                {"touch": "move", "point": armed}
            ])
        } else {
            json!([
                {"point": self.start}, {"wait_ms": 80}, {"down": true}, {"wait_ms": 80},
                {"point": pickup}, {"wait_ms": 80}, {"point": armed}
            ])
        });
        self.begins += 1;
        until(
            || {
                read(&self.input.dir.join("source-begin.json"))
                    .is_some_and(|v| v["count"] == self.begins)
            },
            "native source pickup before crossing windows",
        );
        let begun = read(&self.input.dir.join("source-begin.json")).unwrap();
        assert_eq!(begun["count"], self.begins);
        assert_eq!(begun["device"], if touch { "Touchscreen" } else { "Mouse" });
        let approach = [point[0] - 4., point[1]];
        let device = if touch { "touch" } else { "mouse" };
        self.input.perform(json!([
            contact(device, "move", approach),
            contact(device, "move", point)
        ]));
    }
    fn release(&mut self, touch: bool) {
        self.input.perform(if touch {
            json!([{"touch": "up"}])
        } else {
            json!([{"down": false}])
        });
    }
    fn click_placement(&mut self, w: &Workspace, name: &str) {
        until(
            || find_named(w.window.upcast_ref(), name).is_some_and(|b| b.is_mapped()),
            "the canvas action bar shows the placement action",
        );
        let button = find_named(w.window.upcast_ref(), name).expect("visible placement action");
        assert!(button.is_mapped() && button.is_sensitive(), "{name}");
        let bounds = button.compute_bounds(&w.window).unwrap();
        let point = [
            bounds.x() + bounds.width() * 0.5,
            bounds.y() + bounds.height() * 0.5,
        ];
        self.input.click(point);
        if name != "canvas-bar-PlacementOriginalSize" {
            until(
                || !w.canvas_bar.root.is_visible(),
                "placement controls retire after native click",
            );
        }
    }
}

#[test]
#[ignore = "isolated compositor and native keyboard delivery"]
#[allow(deprecated)]
fn native_multiple_photo_import_chooser() {
    fn file_list(root: &gtk::Widget) -> Option<gtk::Widget> {
        if root.is_mapped() && (root.is::<gtk::ColumnView>() || root.is::<gtk::TreeView>()) {
            return Some(root.clone());
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Some(found) = file_list(&widget) { return Some(found); }
            child = widget.next_sibling();
        }
        None
    }
    let app = native_test_app("art.capycanvas.MultiplePhotoImport");
    let w = Workspace::with_project(&app, Some((new_drawing(200, 150, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let mut driver = FileDrag::start(&w);
    driver.stop_source();
    w.window.present();
    let directory = driver.input.dir.join("chooser-photos");
    std::fs::create_dir(&directory).unwrap();
    let source = super::place_source::source();
    let paths = [directory.join("First photo.png"), directory.join("Second photo.png")];
    for path in &paths {
        layer_color::photo::write_png(std::fs::File::create(path).unwrap(), &source).unwrap();
    }
    let before = ui_session(&w).engine().document().layers.clone();
    for apply in [false, true] {
        invoke(&w, CommandId::ImportImage);
        let chooser = super::new_photo::chooser();
        assert!(chooser.selects_multiple());
        chooser.set_file(&gtk::gio::File::for_path(&paths[0])).unwrap();
        pump(300);
        let list = file_list(chooser.upcast_ref()).expect("visible native chooser file list");
        assert!(list.grab_focus());
        // Select and accept through Mutter's virtual keyboard and Wayland.
        // No callback supplies a synthetic list of selected files to the app.
        driver.input.perform(json!([
            {"key": 0xffe3, "down": true}, {"key": 0x61, "down": true},
            {"key": 0x61, "down": false}, {"key": 0xffe3, "down": false}
        ]));
        until(|| chooser.files().n_items() == 2, "native chooser selects both files");
        let selected: Vec<_> = chooser.files().iter::<gtk::gio::File>()
            .map(|file| file.unwrap().path().unwrap()).collect();
        assert_eq!(selected, paths);
        driver.input.perform(json!([{"key": 0xff0d, "down": true}, {"key": 0xff0d, "down": false}]));
        until(|| !chooser.is_visible(), "native Return accepts the chooser");
        finish(&w);
        ready(&w);
        let imported = ui_session(&w).engine().document().layers.clone();
        let photos: Vec<_> = imported.iter().filter(|layer| layer.source.is_some()).collect();
        assert_eq!(imported.len(), before.len() + 2);
        assert_eq!(photos.iter().map(|layer| layer.name.as_ref()).collect::<Vec<_>>(),
            ["First photo", "Second photo"]);
        for layer in photos {
            assert_eq!(layer.source.as_deref(), Some(&source));
            assert!(layer.raster.is_empty());
        }
        driver.click_placement(&w, if apply { "canvas-bar-ApplyTransform" } else { "canvas-bar-CancelTransform" });
        ready(&w);
        if apply {
            let saved = super::place_source::snapshot(&w);
            let reopened = layer_core::Project::read(saved.as_slice(), Default::default()).unwrap();
            assert_eq!(reopened.document.layers, imported);
            invoke(&w, CommandId::Undo);
            ready(&w);
        }
        assert_eq!(ui_session(&w).engine().document().layers, before);
    }
    println!("native chooser multiple selection: ordered retained sources, Cancel, Apply, save/reopen and one Undo passed");
    driver.input.finish();
    w.window.destroy();
}

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_photo_file_drops"]
fn native_photo_file_drops() {
    let app = native_test_app("art.capycanvas.PhotoFileDrops");
    let mut project = new_drawing(200, 150, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let group = project.document.allocate_layer_id();
    let mut row = layer_core::Layer::paint(group, "Photo destination");
    row.kind = layer_core::LayerKind::Group;
    row.properties.offset = Point { x: 40., y: -10. };
    project.document.layers[0].properties.parent = Some(group);
    project.document.layers.insert(0, row);
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(layer_ui::WorkspaceState::default()),
    });
    pump(300);
    invoke(&w, CommandId::RotateRight);
    invoke(&w, CommandId::ZoomOut);
    ready(&w);
    let dir = PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap());
    let original = ui_session(&w)
        .engine()
        .document()
        .layers
        .clone();
    let mut builder = layer_core::color::source::SourceBuilder::new(
        [1200, 800],
        super::place_source::source().interpretation,
        16 << 20,
    )
    .unwrap();
    for y in 0..800 {
        let row: Vec<u8> = (0..1200)
            .flat_map(|x| [65535u16, (x * 47) as u16, (y * 71) as u16, 65535])
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    let source = builder.finish().unwrap();
    let paths = [dir.join("First photo.png"), dir.join("Second – photo.png")];
    for path in &paths {
        layer_color::photo::write_png(std::fs::File::create(path).unwrap(), &source).unwrap();
    }
    let mut driver = FileDrag::start(&w);
    let area = w.area.compute_bounds(&w.window).unwrap();
    let point = ((w.window.width() / 2 + 40)..(w.window.width() - 40))
        .step_by(20)
        .map(|x| [x as f32, area.y() + area.height() * 0.52])
        .find(|p| {
            w.window
                .pick(p[0] as f64, p[1] as f64, gtk::PickFlags::DEFAULT)
                .is_some_and(|picked| picked == w.area)
        })
        .expect("uncovered canvas to the right of the source window");
    println!(
        "canvas drop point {point:?}, area {area:?}, viewport {:?}",
        state(&w).camera.viewport
    );
    let scale = w.area.scale_factor() as f32;
    let center = state(&w).camera.input_transform().map(Point {
        x: (point[0] - area.x()) * scale,
        y: (point[1] - area.y()) * scale,
    });
    for touch in [false, true] {
        driver.hover(&paths, point, touch);
        assert!(
            w.image_drop_label.is_visible(),
            "native canvas COPY preview"
        );
        driver.release(touch);
        finish(&w);
        ready(&w);
        assert!(!w.image_drop_label.is_visible());
        let doc = ui_session(&w)
            .engine()
            .document()
            .clone();
        assert_eq!(doc.layers.len(), original.len() + 2);
        let photos: Vec<_> = doc.layers.iter().filter(|l| l.source.is_some()).collect();
        assert_eq!(
            photos.iter().map(|l| l.name.as_ref()).collect::<Vec<_>>(),
            ["First photo", "Second – photo"]
        );
        for photo in photos {
            assert_eq!(photo.source.as_deref(), Some(&source));
            let actual = doc
                .layer_geometry(photo.id)
                .map(Point { x: 600., y: 400. }).unwrap();
            assert!(
                (actual.x - center.x).abs() < 0.5 && (actual.y - center.y).abs() < 0.5,
                "drop camera mapping {actual:?} != {center:?}"
            );
            assert!(photo.raster.is_empty());
        }
        assert!(
            state(&w)
                .commands
                .iter()
                .any(|c| c.id == CommandId::ApplyTransform && c.enabled)
        );
        if !touch {
            let report =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/image-placement");
            std::fs::create_dir_all(&report).unwrap();
            super::new_photo::capture_ui(&w, &report, "native-photo-batch.png");
        }
        driver.click_placement(&w, "canvas-bar-ApplyTransform");
        ready(&w);
        let saved = super::place_source::snapshot(&w);
        let reopened = layer_core::Project::read(saved.as_slice(), Default::default()).unwrap();
        assert_eq!(reopened.document.layers, doc.layers);
        invoke(&w, CommandId::Undo);
        ready(&w);
        assert_eq!(
            ui_session(&w)
                .engine()
                .document()
                .layers,
            original
        );
        println!(
            "native external {} canvas batch: source profiles, camera mapping, Apply/save/one undo passed",
            if touch { "touch" } else { "mouse" }
        );
    }
    for (fraction, class, parent) in [
        (0.1, "layer-drop-before", None),
        (0.5, "layer-drop-into", Some(group)),
        (0.9, "layer-drop-after", None),
    ] {
        let row = find_named(
            w.layer_panel.root.upcast_ref(),
            &format!("art-layer-{}", group.0),
        )
        .unwrap();
        let bounds = row.compute_bounds(&w.window).unwrap();
        let target = [
            bounds.x() + bounds.width() * 0.65,
            bounds.y() + bounds.height() * fraction,
        ];
        driver.hover(&paths[..1], target, false);
        assert!(row.has_css_class(class), "native row drop marker {class}");
        driver.release(false);
        finish(&w);
        ready(&w);
        assert!(!row.has_css_class(class));
        let doc = ui_session(&w)
            .engine()
            .document()
            .clone();
        let photo = doc.layer(doc.active_layer).unwrap();
        assert_eq!(photo.properties.parent, parent);
        assert_eq!(
            doc.layer_geometry(photo.id)
                .map(Point { x: 600., y: 400. }).unwrap(),
            Point { x: 100., y: 75. }
        );
        let expected = if fraction < 0.2 {
            0
        } else if fraction < 0.8 {
            1
        } else {
            2
        };
        assert_eq!(
            doc.layers.iter().position(|l| l.id == photo.id),
            Some(expected)
        );
        driver.click_placement(&w, "canvas-bar-CancelTransform");
        ready(&w);
        assert_eq!(
            ui_session(&w)
                .engine()
                .document()
                .layers,
            original
        );
        println!("native external row {class}: destination, group offset and Cancel passed");
    }
    // The first file decodes successfully; failure in the second must publish
    // neither, clear the reservation, and leave no active placement/history.
    let broken = dir.join("Broken photo.png");
    std::fs::write(&broken, b"not a valid photo").unwrap();
    driver.hover(&[paths[0].clone(), broken], point, false);
    driver.release(false);
    until(
        || !state(&w).document_file.busy && state(&w).requests.is_empty() && !w.servicing.get(),
        "failed batch retires worker",
    );
    assert!(
        state(&w)
            .host_error
            .as_deref()
            .is_some_and(|e| e.contains("Broken photo") && e.contains("No images were imported"))
    );
    assert_eq!(
        ui_session(&w)
            .engine()
            .document()
            .layers,
        original
    );
    assert!(
        !state(&w)
            .commands
            .iter()
            .any(|c| c.id == CommandId::ApplyTransform && c.enabled)
    );

    // Cancel through the actual native progress UI before owner publication.
    let cancellations = Rc::new(Cell::new(0));
    let signal = w.window.connect_notify_local(
        Some("visible-dialog"),
        glib::clone!(
            #[strong]
            cancellations,
            move |window, _| {
                let Some(dialog) = window
                    .visible_dialog()
                    .filter(|d| d.widget_name() == "image-import-progress")
                else {
                    return;
                };
                let cancellations = cancellations.clone();
                glib::idle_add_local_full(glib::Priority::HIGH, move || {
                    cancellations.set(cancellations.get() + 1);
                    find_button(dialog.upcast_ref(), "Cancel")
                        .unwrap()
                        .emit_clicked();
                    glib::ControlFlow::Break
                });
            }
        ),
    );
    driver.hover(&paths, point, false);
    driver.release(false);
    finish(&w);
    assert_eq!(cancellations.get(), 1);
    w.window.disconnect(signal);
    assert_eq!(
        ui_session(&w)
            .engine()
            .document()
            .layers,
        original
    );
    assert!(!w.servicing.get(), "Cancel waits for the worker to retire");

    // A queued owner edit can change the destination while the worker runs.
    // Reject its stale result without undoing that independent selection.
    let signal = w.window.connect_notify_local(
        Some("visible-dialog"),
        glib::clone!(
            #[weak]
            w,
            move |window, _| {
                if window
                    .visible_dialog()
                    .is_some_and(|d| d.widget_name() == "image-import-progress")
                {
                    glib::idle_add_local_full(
                        glib::Priority::HIGH,
                        glib::clone!(
                            #[weak]
                            w,
                            #[upgrade_or]
                            glib::ControlFlow::Break,
                            move || {
                                w.dispatch(UiAction::Layer {
                                    action: layer_ui::LayerAction::Select {
                                        id: group.0,
                                        mask: false,
                                    },
                                });
                                glib::ControlFlow::Break
                            }
                        ),
                    );
                }
            }
        ),
    );
    driver.hover(&paths, point, false);
    driver.release(false);
    until(
        || !state(&w).document_file.busy && state(&w).requests.is_empty() && !w.servicing.get(),
        "stale batch retires worker",
    );
    w.window.disconnect(signal);
    assert!(
        state(&w)
            .host_error
            .as_deref()
            .is_some_and(|e| e.contains("changed while importing")),
        "stale destination: {:?}",
        state(&w).host_error
    );
    assert_eq!(
        ui_session(&w)
            .engine()
            .document()
            .active_layer,
        group
    );
    assert_eq!(
        ui_session(&w)
            .engine()
            .document()
            .layers,
        original
    );

    // Project/image mixtures get an explanatory forbidden-drop preview. A
    // single project follows Open and never becomes a layer in this document.
    let native = dir.join("Drawing.capy");
    std::fs::write(&native, super::place_source::snapshot(&w)).unwrap();
    driver.hover(&[native.clone(), paths[0].clone()], point, false);
    assert!(w.image_drop_label.is_visible());
    assert!(
        w.image_drop_label
            .label()
            .contains("batch containing only images")
    );
    driver.release(false);
    assert_eq!(
        ui_session(&w)
            .engine()
            .document()
            .layers,
        original
    );
    let opened = Rc::new(RefCell::new(None));
    *w.open_document.borrow_mut() = Some(Rc::new(glib::clone!(
        #[strong]
        opened,
        move |project, location, _| {
            opened.replace(Some((project, location)));
        }
    )));
    driver.hover(&[native], point, false);
    assert_eq!(w.image_drop_label.label(), "Open drawing");
    driver.release(false);
    finish(&w);
    let (project, location) = opened
        .borrow_mut()
        .take()
        .expect("native project drop invokes Open");
    assert_eq!(project.document.layers, original);
    assert!(location.is_some());
    assert_eq!(
        ui_session(&w)
            .engine()
            .document()
            .layers,
        original
    );
    println!(
        "native external batches: malformed second file, Cancel, stale target, mixed-batch feedback and project Open passed"
    );
    driver.input.finish();
    drop(driver);
    w.window.destroy();
}

#[test]
#[ignore = "private Wayland display, hardware GPU and native pointer/keyboard"]
fn native_photo_transform_reference_pivot_snap_and_nudge() {
    let app = native_test_app("art.capycanvas.TransformReference");
    let mut project = new_drawing(300, 220, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let id = project.document.active_layer;
    let photo = project.document.layers.iter_mut().find(|layer| layer.id == id).unwrap();
    photo.source = Some(layer_core::color::source::rgba8_source([120, 80], |_, _| [40, 120, 200, 255]));
    photo.properties.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine::translation(Point { x: 40., y: 50. }));
    let mut neighbor = layer_core::Layer::paint(project.document.allocate_layer_id(), "Snap reference");
    neighbor.source = Some(layer_core::color::source::rgba8_source([20, 80], |_, _| [200, 80, 40, 255]));
    neighbor.properties.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine::translation(Point { x: 180., y: 50. }));
    project.document.layers.push(neighbor);
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize(); w.window.present(); ready(&w);
    if let Ok(theme) = std::env::var("CAPY_NATIVE_TEST_THEME") {
        w.dispatch(UiAction::SetTheme { theme: Some(if theme == "dark" { layer_ui::Theme::Dark } else { layer_ui::Theme::Light }) });
    }
    for panel in layer_ui::Panel::ALL.into_iter().filter(|panel| !matches!(panel, layer_ui::Panel::Toolbar | layer_ui::Panel::Commands | layer_ui::Panel::ToolSettings)) {
        w.dispatch(UiAction::Customize { action: layer_ui::CustomizationAction::SetPanelVisible { panel, visible: false } });
    }
    invoke(&w, CommandId::FitCanvas); ready(&w);
    invoke(&w, CommandId::ScaleRotate);
    until(|| state(&w).tool_settings.iter().any(|field| field.id == "transform_x"), "reference controls ready");
    let mut native = super::canvas_bar_tests::remote_input();
    let output = PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mapped = |name: &str| widgets(w.window.upcast_ref()).find(|widget| widget.widget_name() == name && widget.is_mapped()).unwrap_or_else(|| panic!("mapped {name}"));
    let number = |name: &str| state(&w).tool_settings.iter().find(|field| field.id == name).unwrap().value;
    let geometry = || ui_session(&w).engine().document().layer_geometry(id);
    let unchanged = geometry();
    for index in [1, 2, 3, 0] {
        let modes = mapped("canvas-bar-choice-transform-mode");
        assert!(modes.has_css_class("linked") && modes.has_css_class("selection-modes"));
        assert_eq!(modes.downcast_ref::<gtk::Box>().unwrap().spacing(), 0);
        let buttons = descendants::<gtk::ToggleButton>(&modes);
        assert_eq!(buttons.len(), 4);
        for pair in buttons.windows(2) {
            let a = pair[0].compute_bounds(&w.window).unwrap();
            let b = pair[1].compute_bounds(&w.window).unwrap();
            assert!((a.x() + a.width() - b.x()).abs() <= 1., "joined mode buttons have no gap");
        }
        native.click(screen_point(buttons[index].upcast_ref(), &w.window, [0.5, 0.5]));
        let buttons = descendants::<gtk::ToggleButton>(&mapped("canvas-bar-choice-transform-mode"));
        assert_eq!(buttons.iter().filter(|button| button.is_active()).count(), 1);
        assert!(buttons[index].is_active());
    }
    assert_eq!(geometry(), unchanged, "changing Transform modes keeps the accepted placement");
    let reference = mapped("tool-choice-bar-transform-reference").compute_bounds(&w.window).unwrap();
    let x = mapped("tool-setting-transform_x").compute_bounds(&w.window).unwrap();
    let y = mapped("tool-setting-transform_y").compute_bounds(&w.window).unwrap();
    assert!((reference.width() - 54.).abs() <= 1. && (reference.height() - 54.).abs() <= 1., "compact nine-dot selector: {reference:?}");
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { native.perform(json!([{"wait_ms":250},{"capture":"transform-position-layout"}])); }
    assert!((x.x() - reference.x() - reference.width() - 12.).abs() <= 1. && (x.x() - y.x()).abs() <= 1., "selector sits beside aligned X/Y fields: selector={reference:?}, X={x:?}, Y={y:?}");
    assert!((reference.y() + reference.height() * 0.5 - (x.y() + y.y() + y.height()) * 0.5).abs() <= 2., "selector is centered beside the X/Y rows");
    for (index, anchor) in layer_ui::CanvasAnchor::ALL.into_iter().enumerate() {
        let widget = mapped(&format!("tool-choice-transform-reference-{index}"));
        let bounds = widget.compute_bounds(&w.window).unwrap();
        assert!((bounds.width() - 18.).abs() <= 1. && (bounds.height() - 18.).abs() <= 1., "compact dot hit target: {bounds:?}");
        let dot = widget.first_child().unwrap().downcast::<gtk::Image>().unwrap();
        assert!(dot.is_visible()); assert_eq!(dot.pixel_size(), 6);
        native.click(screen_point(&widget, &w.window, [0.5, 0.5]));
        let [x, y] = anchor.cell();
        assert!((number("transform_x") - (40. + x as f32 * 60.)).abs() < 0.001);
        assert!((number("transform_y") - (50. + y as f32 * 40.)).abs() < 0.001);
        assert_eq!(geometry(), unchanged, "reference changes are presentation state");
    }
    native.click(screen_point(&mapped("tool-choice-transform-reference-0"), &w.window, [0.5, 0.5]));
    super::new_photo::capture_ui(&w, &output, "transform-reference-grid.png");
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { native.perform(json!([{"wait_ms":250},{"capture":"transform-reference-grid"}])); }
    let type_number = |native: &mut RemoteInput, id: &str, text: &str| {
        let field = mapped(&format!("tool-setting-{id}"));
        if let Some(spin) = descendant::<gtk::SpinButton>(&field) { spin.grab_focus(); }
        else {
            let display = find_css(&field, "number-value").unwrap();
            display.grab_focus(); pump(50);
            native.click(screen_point(&display, &w.window, [0.5, 0.5]));
            descendant::<gtk::Entry>(&field).unwrap().grab_focus();
        }
        native.perform(json!([{"key":0xffe3,"down":true},{"key":97,"down":true},{"key":97,"down":false},{"key":0xffe3,"down":false}]));
        for c in text.chars() { native.key(c as u32); }
        native.key(0xff0d); ready(&w);
    };
    type_number(&mut native, "transform_x", "45");
    assert!((geometry().map(Point::default()).unwrap().x - 45.).abs() < 0.01);
    type_number(&mut native, "transform_x", "40");
    assert_eq!(geometry(), unchanged);
    let pivot = super::canvas_bar_tests::canvas_point(&w, [100., 90.]);
    let custom = super::canvas_bar_tests::canvas_point(&w, [80., 75.]);
    native.perform(json!([{"point":pivot,"down":true},{"point":custom},{"down":false}]));
    assert_eq!(geometry(), unchanged, "moving the pivot does not move pixels");
    type_number(&mut native, "transform_angle", "30");
    let fixed = geometry().map(Point { x: 40., y: 25. }).unwrap();
    assert!((fixed.x - 80.).hypot(fixed.y - 75.) < 0.02, "numeric rotation uses the dragged pivot: {fixed:?}");
    let corner = geometry().map(Point { x: 120., y: 80. }).unwrap();
    let corner = super::canvas_bar_tests::canvas_point(&w, [corner.x, corner.y]);
    native.perform(json!([{"key":0xffe9,"down":true},{"point":corner,"down":true},{"point":[corner[0]+15.,corner[1]+10.]},{"down":false},{"key":0xffe9,"down":false}]));
    let fixed = geometry().map(Point { x: 40., y: 25. }).unwrap();
    assert!((fixed.x - 80.).hypot(fixed.y - 75.) < 0.05, "Alt scaling keeps the dragged pivot: {fixed:?}");
    super::new_photo::capture_ui(&w, &output, "transform-custom-pivot.png");
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { native.perform(json!([{"wait_ms":250},{"capture":"transform-custom-pivot"}])); }
    invoke(&w, CommandId::ResetTransform); ready(&w);
    pump(500);
    invoke(&w, CommandId::TransformSnapping); pump(500);
    assert!(state(&w).commands.iter().find(|command| command.id == CommandId::TransformSnapping).unwrap().selected, "snapping command was accepted");
    let from = super::canvas_bar_tests::canvas_point(&w, [60., 70.]);
    let to = super::canvas_bar_tests::canvas_point(&w, [79., 70.]);
    let edge = super::canvas_bar_tests::canvas_point(&w, [179., 50.]);
    let target = super::canvas_bar_tests::canvas_point(&w, [180., 50.]);
    assert!((edge[0]-target[0]).abs() <= 6., "native snap contact is within its logical-pixel threshold");
    native.perform(json!([{"point":from,"down":true},{"point":to}]));
    let right = geometry().map(Point { x: 120., y: 0. }).unwrap();
    assert!((right.x - 180.).abs() < 0.05, "native body drag snaps to the neighboring source: {right:?}");
    super::new_photo::capture_ui(&w, &output, "transform-snap-guides.png");
    native.perform(json!([{"down":false}]));
    invoke(&w, CommandId::ApplyTransform); ready(&w);
    invoke(&w, CommandId::Move); w.area.grab_focus(); pump(50);
    let before = geometry();
    let checkpoint = ui_session(&w).engine().checkpoint();
    native.perform(json!([{"key":0xff53,"down":true},{"wait_ms":600}]));
    assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint, "held nudge is only a preview");
    assert_ne!(geometry(), before);
    native.perform(json!([{"key":0xff53,"down":false}]));
    let moving = geometry();
    invoke(&w, CommandId::Undo); ready(&w); assert_eq!(geometry(), before);
    invoke(&w, CommandId::Redo); ready(&w); assert_eq!(geometry(), moving);
    w.area.grab_focus(); pump(20);
    let before = geometry();
    native.perform(json!([{"key":0xff51,"down":true}]));
    native.key(0xff1b);
    native.perform(json!([{"key":0xff51,"down":false}]));
    assert_eq!(geometry(), before, "Escape then key release keeps the accepted source");
    w.area.grab_focus(); pump(30);
    native.perform(json!([{"key":0xff53,"down":true}]));
    assert_ne!(geometry(), before);
    w.interact(layer_ui::UiInput::Blur); pump(50);
    let accepted = geometry();
    native.perform(json!([{"key":0xff53,"down":false}]));
    assert_eq!(geometry(), accepted, "Blur commits the held nudge and its later release adds no edit");
    invoke(&w, CommandId::Undo); ready(&w); assert_eq!(geometry(), before);
    invoke(&w, CommandId::Redo); ready(&w); assert_eq!(geometry(), accepted);
    until(|| state(&w).commands.iter().any(|command| command.id == CommandId::TransformAgain && command.enabled), "accepted transform can be repeated");
    let before = geometry();
    let checkpoint = ui_session(&w).engine().checkpoint();
    invoke(&w, CommandId::TransformAgain); ready(&w);
    assert_ne!(geometry(), before);
    assert_ne!(ui_session(&w).engine().checkpoint(), checkpoint);
    invoke(&w, CommandId::Undo); ready(&w); assert_eq!(geometry(), before);
    invoke(&w, CommandId::Redo); ready(&w);
    invoke(&w, CommandId::ScaleRotate); ready(&w);
    super::new_photo::capture_ui(&w, &output, "transform-again-controls.png");
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { native.perform(json!([{"wait_ms":250},{"capture":"transform-again-controls"}])); }
    invoke(&w, CommandId::CancelTransform); ready(&w);
    native.finish(); w.window.destroy(); pump(100);
}
