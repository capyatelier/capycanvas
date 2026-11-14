use super::*;
use layer_core::raster::*;
use serde_json::json;

fn pattern_drawing() -> layer_core::Document {
    use layer_core::color::{RgbSpace, SampleDepth};
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let [width, height] = [4216u32, 3016u32];
    let mut project = new_drawing(width, height, &localization).unwrap();
    composition_mut(&mut project).color.space = RgbSpace::Srgb;
    composition_mut(&mut project).color.depth = SampleDepth::U16;
    let descriptor = project.composition().color.paint_descriptor();
    let mut tiles = RasterTiles::default();
    for ty in 0..height.div_ceil(256) {
        for tx in 0..width.div_ceil(256) {
            let mut bytes = Vec::with_capacity(256 * 256 * 8);
            for py in 0..256u32 {
                for px in 0..256u32 {
                    let [x, y] = [tx * 256 + px, ty * 256 + py];
                    let rgb = if px < 4 || py < 4 {
                        [3000u16; 3]
                    } else {
                        [
                            (6000 + u64::from(x) * 50000 / u64::from(width)) as u16,
                            (6000 + u64::from(y) * 50000 / u64::from(height)) as u16,
                            if (x / 16 + y / 16) % 2 == 0 { 12000 } else { 52000 },
                        ]
                    };
                    for v in rgb {
                        bytes.extend_from_slice(&v.to_le_bytes());
                    }
                    bytes.extend_from_slice(&65535u16.to_le_bytes());
                }
            }
            tiles.insert(
                TileKey { plane: RasterPlane::Color, coordinate: [tx, ty] },
                RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()),
            );
        }
    }
    active_paint_mut(&mut project).raster = RasterRevision::backed(RasterData { tiles, ..Default::default() });
    let working = project.working.clone();
    project = layer_core::Document::from_artwork(project.artwork.clone()).unwrap();
    project.working = working;
    project
}

fn settle(w: &Rc<Workspace>) {
    new_photo::ready(w);
    until(|| ui_session(w).frame_demand() == layer_render::FrameDemand::Wait, "presentation settled");
}

fn zoom_and_settle(w: &Rc<Workspace>,zoom:f32) {
    let started=std::time::Instant::now();
    w.dispatch(UiAction::SetZoom {zoom});settle(w);
    let elapsed=started.elapsed();
    assert!(elapsed<=std::time::Duration::from_secs(5),"zoom {zoom} must settle within5s: {elapsed:?}");
    eprintln!("wrong-tile zoom {zoom}: settled in {elapsed:?}");
}

fn record_strokes(w: &Rc<Workspace>, input: &mut RemoteInput, output: &std::path::Path, name: &str, color: [f32; 4], change: impl FnOnce(&Rc<Workspace>)) {
    w.dispatch(UiAction::SetColor { rgba: color });
    pump(100);
    input.perform(json!([{"record_start": name}, {"wait_ms": 150}]));
    let started_ns = glib::monotonic_time() as u64 * 1000;
    change(w);
    let changed_ns = glib::monotonic_time() as u64 * 1000;
    let before = ui_session(w).engine().metrics().committed_strokes;
    let camera = state(w).camera;
    let [aw, ah] = [w.area.width() as f32, w.area.height() as f32];
    let origin = w.area.compute_point(&w.window, &gtk::graphene::Point::zero()).unwrap();
    let size = ui_session(w).engine().document().composition().size;
    let corners = [super::photo_edit::window_point(w, [0., 0.]), super::photo_edit::window_point(w, [size[0] as f32, size[1] as f32])];
    let rect = [corners[0][0].max(370.), corners[0][1].max(110.), corners[1][0].min(1290.), corners[1][1].min(950.)];
    let window = |fx: f32, fy: f32| [rect[0] + fx * (rect[2] - rect[0]), rect[1] + fy * (rect[3] - rect[1])];
    let mut events = Vec::new();
    let mut paths = Vec::new();
    for fy in [0.15f32, 0.38, 0.62, 0.85] {
        let path: Vec<[f32; 2]> = (0..=32)
            .map(|i| {
                let t = i as f32 / 32.;
                window(0.06 + 0.88 * t, fy + 0.06 * (t * std::f32::consts::TAU).sin())
            })
            .collect();
        events.push(json!({"pen": "move", "point": path[0]}));
        for (i, p) in path.iter().enumerate() {
            events.push(json!({"pen": if i == 0 { "down" } else { "move" }, "point": p}));
            events.push(json!({"wait_ms": 4}));
        }
        events.push(json!({"pen": "up"}));
        events.push(json!({"wait_ms": 180}));
        paths.push(path);
    }
    events.push(json!({"pen": "leave"}));
    events.push(json!({"wait_ms": 300}));
    input.perform(json!(events));
    let strokes_ns = glib::monotonic_time() as u64 * 1000;
    settle(w);
    pump(400);
    let settled_ns = glib::monotonic_time() as u64 * 1000;
    input.perform(json!([{"record_stop": true}]));
    let committed = ui_session(w).engine().metrics().committed_strokes;
    let monitor = w.area.display().monitor_at_surface(&w.window.surface().unwrap()).unwrap().geometry();
    let (wx, wy) = w.window.surface_transform();
    let meta = json!({
        "name": name,
        "color": color,
        "started_ns": started_ns, "changed_ns": changed_ns, "strokes_done_ns": strokes_ns, "settled_ns": settled_ns,
        "committed_before": before, "committed_after": committed,
        "monitor": [monitor.x(), monitor.y(), monitor.width(), monitor.height()],
        "window_offset": [wx, wy],
        "area_origin": [origin.x(), origin.y()], "area_size": [aw, ah], "stroke_rect": rect,
        "scale_factor": w.area.scale_factor(),
        "zoom": camera.zoom,
        "brush_size": state(w).brush.diameter,
        "paths": paths,
        "input_step": input.step - 2,
        "host_error": format!("{:?}", state(w).host_error),
    });
    let path=output.join(name).join("meta.json");
    glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        std::fs::write(path,serde_json::to_vec_pretty(&meta).unwrap()).unwrap();
    })).unwrap();
    assert_eq!(committed,before+4,"four recorded strokes commit");
    eprintln!("wrong-tile scenario {name}: committed {before} -> {committed}, zoom {}", camera.zoom);
}

#[test]
#[ignore = "private 1600x1000 scale-1 compositor: workspace-motion.sh gtk --native-test=native_wrong_tile_strokes --tablet; native event 4ms and input trace"]
fn native_wrong_tile_strokes() {
    let output=std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").unwrap());
    let app=native_test_app("art.capycanvas.WrongTileStrokes");
    let mut input=RemoteInput::new().settle_ms(0).timeout_secs(120);
    for fixture in ["pattern","photo"] {for theme in [Theme::Light,Theme::Dark] {
        let project=glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move ||match fixture {
            "photo"=>super::native_navigation::photo([6000,4000]),_=>pattern_drawing(),
        })).unwrap();
        let w=Workspace::with_project(&app,Some((project,None)));
        apply_fixture_theme(&w);w.dispatch(UiAction::SetTheme{theme:Some(theme)});
        w.window.maximize();w.window.present();new_photo::ready(&w);
        let monitor=w.area.display().monitor_at_surface(&w.window.surface().unwrap()).unwrap().geometry();
        assert_eq!([monitor.width(),monitor.height()],[1600,1000],"recorded stroke geometry");
        assert_eq!(w.area.scale_factor(),1,"recorded stroke geometry");
        w.dispatch(UiAction::Invoke{command:CommandId::FitCanvas});
        w.dispatch(UiAction::SelectBrush{id:layer_core::DefaultBrushPreset::GPen as u32});
        w.dispatch(UiAction::SetBrushSize{value:60.});settle(&w);input.ready();pump(500);
        let name=|scenario:&str|format!("{fixture}-{theme:?}-{scenario}").to_lowercase();
        record_strokes(&w,&mut input,&output,&name("fit"),[1.,0.,1.,1.],|_|{});
        zoom_and_settle(&w,1.);
        record_strokes(&w,&mut input,&output,&name("zoom100"),[0.,0.8,1.,1.],|_|{});
        zoom_and_settle(&w,0.5);
        record_strokes(&w,&mut input,&output,&name("zoom50"),[1.,0.9,0.,1.],|_|{});
        record_strokes(&w,&mut input,&output,&name("quickzoom"),[0.1,0.1,0.1,1.],|w|{
            w.dispatch(UiAction::SetZoom{zoom:0.25});pump(30);
        });
        w.dispatch(UiAction::Invoke{command:CommandId::FitCanvas});settle(&w);
        record_strokes(&w,&mut input,&output,&name("fit-again"),[0.,0.7,0.2,1.],|_|{});
        w.window.destroy();pump(100);
    }}
    input.finish();
    let tool=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/performance/gtk-wrong-tiles.py");
    let input=input.dir.clone();
    let status=glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        std::process::Command::new("python3").arg(tool).arg(output).arg(input).status().unwrap()
    })).unwrap();
    assert!(status.success(),"no stale stroke blocks beyond 60ms in pattern/photo, light/dark");
}
