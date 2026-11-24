use super::*;
use gtk::gio;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

const PHOTO_WORKSPACE: &str = "builtin:workspace:photographer";

fn now() -> u64 {
    glib::monotonic_time() as u64 * 1000
}

enum Stop {
    Skip(String),
    Fail(String),
}
type Outcome<T = ()> = Result<T, Stop>;

fn skip<T>(reason: impl Into<String>) -> Outcome<T> {
    Err(Stop::Skip(reason.into()))
}
fn fail<T>(reason: impl Into<String>) -> Outcome<T> {
    Err(Stop::Fail(reason.into()))
}

fn alias(name: &str) -> &str {
    match name {
        "transform" => "scale_rotate",
        "apply" => "apply_transform",
        "magic_wand" => "auto_select",
        other => other,
    }
}

fn guarantee_size(pixels: u64) -> f64 {
    if pixels < 20_000_000 { 1024. } else if pixels < 40_000_000 { 1536. } else { 2048. }
}

fn stroke_point(zigzag: bool, progress: f64, duration_ms: f64) -> [f64; 2] {
    let t = progress * duration_ms / 1000.;
    if zigzag {
        [0.5 + 0.3 * (1. - 4. * (t % 1. - 0.5).abs()), 0.5 + 0.2 * (2. * progress - 1.)]
    } else {
        [0.5 + 0.3 * (2. * std::f64::consts::PI * t).cos(), 0.5 + 0.2 * (2. * std::f64::consts::PI * t).sin()]
    }
}

fn stroke_pressure(progress: f64, duration_ms: f64) -> f64 {
    0.625 - 0.375 * (2. * std::f64::consts::PI * progress * duration_ms / 1000.).cos()
}

fn gesture_points(kind: &str, area: [f64; 4], wave: f64, flick: f64) -> [[f64; 2]; 2] {
    let radius = 0.12 * area[2].min(area[3]);
    let center = [area[0] + area[2] * (0.5 + if kind == "pan" { 0.3 * wave } else { 0. }), area[1] + area[3] * 0.5];
    let spread = radius * if kind == "pinch" { 1. + 1.5 * wave } else { 1. } * (1. + 0.6 * flick);
    let angle = if kind == "rotate" { std::f64::consts::PI / 3. * wave } else { 0. };
    [-1., 1.].map(|d| [center[0] + d * spread * angle.cos(), center[1] + d * spread * angle.sin()])
}

fn samples(duration_ms: f64, event_ms: f64, count: usize) -> usize {
    ((duration_ms / event_ms / count as f64).round() as usize).max(1)
}

#[test]
fn bench_chain_paths_cover_the_work_area() {
    assert_eq!([guarantee_size(12_000_000), guarantee_size(36_000_000), guarantee_size(61_000_000)], [1024., 1536., 2048.]);
    for zigzag in [false, true] {
        for i in 0..=100 {
            let [x, y] = stroke_point(zigzag, i as f64 / 100., 1250.);
            assert!((0.2..=0.8).contains(&x) && (0.3..=0.7).contains(&y), "{zigzag} {i}: {x} {y}");
        }
    }
    let pressures: Vec<_> = (0..=100).map(|i| stroke_pressure(i as f64 / 100., 1250.)).collect();
    assert!(pressures.iter().all(|p| (0.25..=1.).contains(p)));
    assert!(pressures.iter().any(|p| *p > 0.9) && pressures.iter().any(|p| *p < 0.35));
    let area = [100., 50., 800., 600.];
    for kind in ["pan", "pinch", "rotate"] {
        for wave in [0., 0.5, 1.] {
            for point in gesture_points(kind, area, wave, 0.) {
                assert!(point[0] > area[0] && point[0] < area[0] + area[2] && point[1] > area[1] && point[1] < area[1] + area[3]);
            }
        }
    }
    let [a, b] = gesture_points("pinch", area, 1., 0.);
    assert!((b[0] - a[0] - 2. * 0.12 * 600. * 2.5).abs() < 1e-9);
    assert_eq!(samples(1250., 4., 4), 78);
}

struct Opened {
    tab: u64,
    paint: Option<u64>,
    photo: Option<u64>,
}

struct Bench {
    chain: Value,
    files: HashMap<String, std::path::PathBuf>,
    input: RemoteInput,
    event_ms: f64,
    app: Option<NativeTestApp>,
    w: Option<Rc<Workspace>>,
    record: Value,
    refresh_hz: f64,
    scale: Option<f64>,
    canvas: Option<[u32; 2]>,
    start_workspace: Option<String>,
    documents: HashMap<String, Opened>,
    shown: Option<u64>,
    renderers: Vec<Arc<Mutex<crate::timing::Stats>>>,
    window_frames: Rc<RefCell<BTreeMap<i64, u64>>>,
}

impl Bench {
    fn w(&self) -> Outcome<Rc<Workspace>> {
        match &self.w {
            Some(w) if w.window.is_visible() => Ok(w.clone()),
            _ => fail("The app has no open window"),
        }
    }
    fn extra(&mut self) -> &mut serde_json::Map<String, Value> {
        self.record["extra"].as_object_mut().unwrap()
    }
    fn snapshot(&self) -> Outcome<Value> {
        let w = self.w()?;
        let value = serde_json::to_value(ui_session(&w).state()).map_err(|e| Stop::Fail(e.to_string()))?;
        Ok(value)
    }
    fn motion_ms(&self, step: &Value) -> f64 {
        step["duration_ms"].as_f64().unwrap_or_else(|| {
            step["frames"].as_f64().or(self.chain["motion_frames"].as_f64()).unwrap_or(150.) * 1000. / self.refresh_hz
        })
    }
    fn collect_renderer(&mut self) {
        let Some(w) = &self.w else { return };
        let Some(stats) = w.gpu.borrow().as_ref().map(|g| g.session.engine().backend().stats.clone()) else { return };
        if !self.renderers.iter().any(|known| Arc::ptr_eq(known, &stats)) {
            self.renderers.push(stats);
        }
    }

    fn pending(w: &Workspace) -> bool {
        w.gpu.borrow().as_ref().is_none_or(|g| {
            !g.session.engine().backend().frames_idle() || g.session.engine().has_pending_document_edits()
        })
    }
    fn prompt(w: &Workspace) -> Option<String> {
        w.window.visible_dialog().map(|dialog| dialog.widget_name().to_string()).filter(|name| !name.ends_with("-progress"))
    }
    fn quiet(w: &Rc<Workspace>) -> bool {
        w.window.visible_dialog().is_none()
            && !w.documents.has_pending_open()
            && !w.documents.changing.get()
            && !w.documents.paused.get()
            && w.workspaces.ready()
            && !w.workspaces.busy()
            && w.workspaces.accepts_input(w)
            && w.gpu.borrow().as_ref().is_some_and(|g| {
                let engine = g.session.engine();
                engine.metrics().frames > 0
                    && engine.backend().startup.complete
                    && engine.backend().paint_ready(engine.document(), engine.configured_brush(), engine.transform_preview().is_some())
                    && !g.session.state().filter_load.pending
            })
    }
    fn check_host(&self) -> Outcome {
        let w = self.w()?;
        if let Some(error) = ui_session(&w).state().host_error.clone() {
            return fail(error);
        }
        if let Some(dialog) = Self::prompt(&w) {
            return fail(format!("A {dialog} dialog is open"));
        }
        Ok(())
    }
    fn wait_until(&mut self, timeout_ms: u64, mut ready: impl FnMut(&Self) -> Outcome<bool>) -> Outcome<Option<u64>> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let stable = (3e9 / self.refresh_hz) as u64;
        let mut since = None;
        self.shown = None;
        loop {
            let w = self.w()?;
            self.collect_renderer();
            let session = w.gpu.borrow().is_some();
            let met = session && ready(self)?;
            let at = now();
            if met && self.shown.is_none() {
                self.shown = Some(at);
            }
            if met && Self::quiet(&w) && !Self::pending(&w) {
                let idle = *since.get_or_insert(at);
                if at - idle >= stable {
                    return Ok(Some(idle));
                }
            } else {
                since = None;
            }
            if Instant::now() > deadline {
                return Ok(None);
            }
            pump(2);
        }
    }
    fn wait(&mut self, label: &str, timeout_ms: u64, ready: impl FnMut(&Self) -> Outcome<bool>) -> Outcome<u64> {
        match self.wait_until(timeout_ms, ready)? {
            Some(settled) => Ok(settled),
            None => {
                let dialog = self.w.as_ref().and_then(|w| w.window.visible_dialog()).map(|d| format!(" with the {} dialog open", d.widget_name()));
                fail(format!("Timed out waiting for {label}{}{}", if self.shown.is_some() { " (shown, not settled)" } else { "" }, dialog.unwrap_or_default()))
            }
        }
    }
    fn settle(&mut self, label: &str) -> Outcome<u64> {
        let settled = self.wait(label, 60_000, |_| Ok(true));
        self.shown = None;
        settled
    }
    fn latency(&mut self, name: &str, input: u64, settled: u64, surface: &str) {
        let ready = self.shown.take().unwrap_or(settled);
        self.record["latency"].as_array_mut().unwrap().push(json!({
            "name": name, "input_ns": input, "ready_ns": ready, "settled_ns": settled, "surface": surface,
        }));
    }
    fn dispatch(&self, action: Value) -> Outcome {
        let parsed: UiAction = serde_json::from_value(action.clone())
            .map_err(|_| Stop::Skip(format!("this build has no {action} action")))?;
        self.w()?.dispatch(parsed);
        self.check_host()
    }
    fn command(&self, name: &str) -> Outcome {
        let id = alias(name);
        let Ok(command) = serde_json::from_value::<CommandId>(json!(id)) else {
            return skip(format!("command {id} is not in this build"));
        };
        let w = self.w()?;
        if !ui_session(&w).command(command).enabled {
            return fail(format!("command {id} is disabled"));
        }
        w.dispatch(UiAction::Invoke { command });
        self.check_host()
    }
    fn command_enabled(&self, name: &str) -> Outcome<bool> {
        let command: CommandId = serde_json::from_value(json!(alias(name))).map_err(|_| Stop::Skip(format!("command {name} is not in this build")))?;
        Ok(ui_session(&*self.w()?).command(command).enabled)
    }
    fn revision(&self) -> Outcome<u64> {
        Ok(ui_session(&*self.w()?).state().document_file.revision)
    }

    fn origin(w: &Workspace) -> [f64; 2] {
        let point = w.surface.compute_point(&w.window, &gtk::graphene::Point::new(0., 0.)).unwrap_or_else(|| gtk::graphene::Point::new(0., 0.));
        [point.x() as f64, point.y() as f64]
    }
    fn area(&self) -> Outcome<[f64; 4]> {
        let w = self.w()?;
        let b = w.resolved().work_area;
        let [x, y] = Self::origin(&w);
        if b.width < 64. || b.height < 64. {
            return fail(format!("The canvas work area is {} × {}", b.width, b.height));
        }
        Ok([x + b.x as f64, y + b.y as f64, b.width as f64, b.height as f64])
    }
    fn widget_center(w: &Workspace, widget: &gtk::Widget) -> Outcome<[f64; 2]> {
        let Some(b) = widget.compute_bounds(&w.window) else { return fail(format!("{} has no bounds", widget.widget_name())) };
        Ok([(b.x() + b.width() / 2.) as f64, (b.y() + b.height() / 2.) as f64])
    }

    fn perform(&mut self, events: Vec<Value>) -> Vec<u64> {
        let step = self.input.step;
        self.input.perform(Value::Array(events));
        let trace = std::fs::read(self.input.dir.join(format!("trace-{step}.json"))).ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok()).unwrap_or_default();
        trace.iter().filter(|row| !row["event"].get("wait_ms").is_some()).filter_map(|row| row["ns"].as_u64()).collect()
    }
    fn contacts(&mut self, pen: bool, duration_ms: f64, count: usize, pressure: impl Fn(f64) -> f64, point: impl Fn(f64) -> [f64; 2]) -> Vec<[u64; 2]> {
        let samples = samples(duration_ms, self.event_ms, count);
        let mut windows = Vec::new();
        for contact in 0..count {
            let mut events = Vec::new();
            for sample in 0..=samples {
                let progress = (contact as f64 + sample as f64 / samples as f64) / count as f64;
                let at = point(progress);
                let phase = if sample == 0 { "down" } else { "move" };
                events.push(if pen {
                    json!({ "pen": phase, "point": at, "pressure": pressure(progress) })
                } else {
                    json!({ "touch": phase, "point": at })
                });
            }
            events.push(if pen { json!({ "pen": "up" }) } else { json!({ "touch": "up" }) });
            if pen {
                events.push(json!({ "pen": "leave" }));
            }
            let times = self.perform(events);
            let up = times.get(samples + 1).or(times.last()).copied().unwrap_or_else(now);
            self.extra().entry("samples_ns").or_insert_with(|| json!([])).as_array_mut().unwrap()
                .extend(times.iter().take(samples + 1).map(|t| json!(t)));
            windows.push([times.first().copied().unwrap_or(up), up]);
        }
        windows
    }
    fn tap(&mut self, point: [f64; 2], pen: bool) -> u64 {
        let events = if pen {
            vec![json!({ "pen": "down", "point": point, "pressure": 1. }), json!({ "wait_ms": 30 }), json!({ "pen": "up" }), json!({ "pen": "leave" })]
        } else {
            vec![json!({ "touch": "down", "point": point }), json!({ "wait_ms": 30 }), json!({ "touch": "up" })]
        };
        self.perform(events).first().copied().unwrap_or_else(now)
    }
    fn key(&mut self, key: u32) {
        self.perform(vec![json!({ "key": key, "down": true }), json!({ "key": key, "down": false })]);
    }
    fn motion(&mut self, window: [u64; 2]) {
        self.record["motion"].as_array_mut().unwrap().push(json!({ "begin_ns": window[0], "end_ns": window[1] }));
    }
    fn per_frame(&mut self, frames: usize, action: impl Fn(usize) -> Value + 'static) -> Outcome<[u64; 2]> {
        let w = self.w()?;
        let progress = Rc::new(RefCell::new((0usize, 0u64, None::<String>)));
        let state = progress.clone();
        let target = w.clone();
        w.area.add_tick_callback(move |_, _| {
            let mut state = state.borrow_mut();
            if state.0 == 0 {
                state.1 = now();
            }
            match serde_json::from_value::<UiAction>(action(state.0)) {
                Ok(parsed) => target.dispatch(parsed),
                Err(error) => state.2 = Some(error.to_string()),
            }
            state.0 += 1;
            if state.0 < frames && state.2.is_none() { glib::ControlFlow::Continue } else { glib::ControlFlow::Break }
        });
        let deadline = Instant::now() + Duration::from_secs(120);
        while progress.borrow().0 < frames && progress.borrow().2.is_none() {
            if Instant::now() > deadline {
                return fail("Frame-paced changes did not finish");
            }
            pump(1);
        }
        let end = now();
        if let Some(error) = progress.borrow().2.clone() {
            return skip(format!("this build has no such action: {error}"));
        }
        self.check_host()?;
        let begin = progress.borrow().1;
        Ok([begin, end])
    }

    fn editing_layer(&self) -> Outcome<Option<u64>> {
        Ok(self.snapshot()?["layer_properties"]["layer"].as_u64())
    }
    fn active_document(&self) -> Option<&Opened> {
        let selected = self.w.as_ref()?.documents.selected();
        self.documents.values().find(|opened| opened.tab == selected)
    }
    fn select_layer(&mut self, label: Option<&str>) -> Outcome {
        let opened = self.active_document();
        let id = match label {
            None => match opened.and_then(|o| o.paint) { Some(id) => Some(id), None => return Ok(()) },
            Some("Photo") => opened.and_then(|o| o.photo),
            Some(_) => None,
        };
        let id = match (id, label) {
            (Some(id), _) => id,
            (None, Some(label)) => {
                let w = self.w()?;
                let found = ui_session(&w).state().layers.iter().find(|layer| layer.label.as_str() == label).map(|layer| layer.id);
                match found { Some(id) => id, None => return skip(format!("no layer labelled {label}")) }
            }
            (None, None) => return Ok(()),
        };
        self.dispatch(json!({ "type": "select_layer", "id": id }))
    }
    fn active_pixels(&self) -> Outcome<u64> {
        let w = self.w()?;
        let state = ui_session(&w);
        let tab = state.state().tabs.iter().find(|tab| tab.active).map(|tab| [tab.width, tab.height]);
        match tab { Some([width, height]) => Ok(width as u64 * height as u64), None => fail("No active drawing tab") }
    }

    fn launch(&mut self) -> Outcome {
        let input = now();
        crate::start_process();
        gtk::init().map_err(|e| Stop::Fail(e.to_string()))?;
        let (app, active) = crate::application(crate::storage::APP_ID);
        app.register(None::<&gio::Cancellable>).map_err(|e| Stop::Fail(e.to_string()))?;
        self.app = Some(NativeTestApp(app.clone()));
        app.activate();
        let deadline = Instant::now() + Duration::from_secs(120);
        let w = loop {
            if let Some(w) = active.borrow().last().filter(|w| w.window.is_visible()).cloned() { break w; }
            if Instant::now() > deadline { return fail("The application opened no window"); }
            pump(2);
        };
        self.w = Some(w.clone());
        let ready = self.wait("a ready brush", 120_000, |bench| Ok(Self::quiet(&bench.w()?)))?;
        self.latency("launch", input, ready, "canvas");
        w.window.maximize();
        self.settle("the maximized window")?;
        let frames = self.window_frames.clone();
        if let Some(clock) = w.window.frame_clock() {
            clock.connect_after_paint(move |clock| {
                let mut frames = frames.borrow_mut();
                let current = clock.frame_counter();
                for counter in current.saturating_sub(12)..current {
                    if frames.contains_key(&counter) { continue; }
                    if let Some(timings) = clock.timings(counter).filter(|t| t.is_complete() && t.presentation_time() > 0) {
                        frames.insert(counter, timings.presentation_time() as u64 * 1000);
                    }
                }
            });
        }
        let surface = w.window.surface();
        self.scale = surface.as_ref().map(|s| s.scale());
        self.refresh_hz = surface.as_ref().and_then(|s| s.display().monitor_at_surface(s)).map(|m| m.refresh_rate() as f64 / 1000.)
            .filter(|hz| *hz >= 30.).unwrap_or(60.);
        self.start_workspace = w.workspaces.view().id;
        let tabs = w.documents.len();
        let start = self.start_workspace.clone();
        self.extra().extend([("workspace".into(), json!(start)), ("tabs".into(), json!(tabs))]);
        Ok(())
    }

    fn open(&mut self, name: &str) -> Outcome {
        self.record["input"] = json!("dispatch");
        let Some(file) = self.files.get(name).cloned() else { return skip("fixture not provided") };
        let w = self.w()?;
        let (selected, count) = (w.documents.selected(), w.documents.len());
        let input = now();
        crate::files::launch::open_in(&w, vec![gio::File::for_path(&file)]);
        let ready = self.wait(&format!("{name} to open"), 120_000, |bench| {
            let w = bench.w()?;
            if let Some(dialog) = Self::prompt(&w) {
                return fail(format!("Opening {name} showed the {dialog} dialog"));
            }
            if let Some(error) = ui_session(&w).state().host_error.clone() {
                return fail(error);
            }
            Ok((w.documents.selected() != selected || w.documents.len() != count) && Self::quiet(&w))
        })?;
        self.latency("open", input, ready, "canvas");
        let photo = if self.chain["documents"][name]["kind"] == "photo" { self.editing_layer()? } else { None };
        if photo.is_some() {
            self.command("add_layer")?;
            self.settle("the paint layer")?;
        }
        let paint = self.editing_layer()?;
        let tab = w.documents.selected();
        self.documents.insert(name.into(), Opened { tab, paint, photo });
        let pixels = {
            let state = ui_session(&w);
            state.state().tabs.iter().find(|t| t.active).map(|t| [t.width, t.height])
        };
        self.canvas = self.canvas.or(pixels);
        self.extra().insert("canvas".into(), json!(pixels));
        Ok(())
    }

    fn stroke(&mut self, step: &Value) -> Outcome {
        let preset = step["preset"].as_u64().unwrap_or_default() as u32;
        self.select_layer(step["layer"].as_str())?;
        self.dispatch(json!({ "type": "select_brush", "id": preset }))?;
        if ui_session(&*self.w()?).state().brush.preset != preset {
            return skip(format!("brush preset {preset} is not in this build"));
        }
        self.settle(&format!("brush {preset}"))?;
        let tool = self.snapshot()?["brush"]["tool"].as_str().unwrap_or_default().to_owned();
        if matches!(tool.as_str(), "clone" | "heal" | "spot_heal") {
            self.command("use_reference_below")?;
        }
        let size = match &step["size"] {
            Value::String(text) if text == "guarantee" => guarantee_size(self.active_pixels()?),
            value => value.as_f64().unwrap_or(100.),
        };
        self.dispatch(json!({ "type": "set_brush_size", "value": size }))?;
        match &step["zoom"] {
            Value::String(text) if text == "fit" => self.command("fit_canvas")?,
            Value::Number(zoom) => self.dispatch(json!({ "type": "set_zoom", "zoom": zoom }))?,
            _ => {}
        }
        self.settle("brush ready")?;
        let (diameter, zoom) = {
            let w = self.w()?;
            let state = ui_session(&w);
            (state.state().brush.diameter, state.state().camera.zoom)
        };
        self.extra().extend([("size".into(), json!(diameter)), ("zoom".into(), json!(zoom)), ("tool".into(), json!(tool))]);
        let a = self.area()?;
        let at = |fx: f64, fy: f64| [a[0] + a[2] * fx, a[1] + a[3] * fy];
        if matches!(tool.as_str(), "clone" | "heal") {
            self.command("clone_source_arm")?;
            self.tap(at(0.25, 0.5), true);
            self.wait("clone source", 30_000, |bench| Ok(!bench.command_selected("clone_source_arm")?))?;
        }
        let primed = self.revision()?;
        let travel = if tool == "liquify" { 0.025 } else { 0. };
        self.contacts(true, 100., 1, |_| 1., |p| at(0.5 + travel * p, 0.5));
        self.wait("priming dab", 30_000, |bench| Ok(bench.revision()? > primed))?;
        self.extra().remove("samples_ns");
        let duration = self.motion_ms(step);
        let zigzag = step["path"] == "zigzag";
        let count = step["contacts"].as_u64().unwrap_or(1).max(1) as usize;
        let windows = self.contacts(true, duration, count, |p| stroke_pressure(p, duration), |p| {
            let [x, y] = stroke_point(zigzag, p, duration);
            at(x, y)
        });
        let settled = self.wait_until(step["catch_up_cap_ms"].as_u64().unwrap_or(3000), |_| Ok(true))?;
        self.shown = None;
        self.record["settled_ns"] = json!(settled);
        if settled.is_none() {
            self.extra().insert("catch_up_capped".into(), json!(true));
        }
        self.settle("stroke catch-up")?;
        for window in windows {
            self.motion(window);
            self.record["contacts"].as_array_mut().unwrap().push(json!({ "down_ns": window[0], "up_ns": window[1] }));
        }
        Ok(())
    }
    fn command_selected(&self, name: &str) -> Outcome<bool> {
        Ok(self.snapshot()?["commands"].as_array().into_iter().flatten()
            .any(|c| c["id"] == alias(name) && c["selected"].as_bool() == Some(true)))
    }

    fn gesture(&mut self, step: &Value) -> Outcome {
        let kind = step["gesture"].as_str().unwrap_or_default().to_owned();
        if !matches!(kind.as_str(), "pan" | "pinch" | "rotate") {
            return skip(format!("unknown gesture {kind}"));
        }
        let quick = step["release"] == "quick";
        let a = self.area()?;
        let before = ui_session(&*self.w()?).state().camera.revision;
        let duration = self.motion_ms(step);
        let frames = samples(duration, 2. * self.event_ms, 1);
        let flick = if quick { samples(100., 2. * self.event_ms, 1) } else { 0 };
        let at = |i: usize| {
            let s = (i as f64 / frames as f64).min(1.);
            let f = if flick == 0 { 0. } else { (i as f64 - frames as f64).clamp(0., flick as f64) / flick as f64 };
            gesture_points(&kind, a, (std::f64::consts::PI * s).sin(), f)
        };
        let first = at(0);
        let mut events = vec![json!({ "touch": "down", "slot": 0, "point": first[0] }), json!({ "touch": "down", "slot": 1, "point": first[1] })];
        for i in 1..=frames + flick {
            let points = at(i);
            events.extend((0..2).map(|slot| json!({ "touch": "move", "slot": slot, "point": points[slot] })));
        }
        events.extend([json!({ "touch": "up", "slot": 1 }), json!({ "touch": "up", "slot": 0 })]);
        let times = self.perform(events);
        self.motion([times.first().copied().unwrap_or_else(now), times.last().copied().unwrap_or_else(now)]);
        if quick {
            let deadline = Instant::now() + Duration::from_secs(30);
            let w = self.w()?;
            let mut previous = None;
            loop {
                let camera = ui_session(&w).state().camera.clone();
                if previous.as_ref() == Some(&camera) && !Self::pending(&w) { break; }
                if Instant::now() > deadline { return fail("The camera did not stop"); }
                previous = Some(camera);
                pump(4);
            }
            self.record["settled_ns"] = json!(now());
        }
        self.settle("gesture to settle")?;
        if ui_session(&*self.w()?).state().camera.revision == before {
            return fail(format!("The {kind} gesture did not move the camera"));
        }
        Ok(())
    }

    fn fill_point(&self, target: [f64; 2]) -> Outcome<[f64; 2]> {
        let w = self.w()?;
        let camera = ui_session(&w).state().camera.clone();
        let [width, height] = self.canvas_of_active()?;
        let scale = w.window.surface().map_or(1., |s| s.scale());
        let [ox, oy] = Self::origin(&w);
        let physical = [(target[0] - ox) * scale, (target[1] - oy) * scale];
        let m = camera.input_transform().surface_to_document.map(f64::from);
        let doc = [m[0] * physical[0] + m[2] * physical[1] + m[4], m[1] * physical[0] + m[3] * physical[1] + m[5]];
        let (hx, hy) = (width as f64 / 2., height as f64 / 2.);
        let reach = ((doc[0] - hx) / hx).abs().max(((doc[1] - hy) / hy).abs());
        if reach < 1. { return Ok(target); }
        let projected = [hx + (doc[0] - hx) * 0.9 / reach, hy + (doc[1] - hy) * 0.9 / reach];
        let d = camera.document_to_surface().map(f64::from);
        let surface = [d[0] * projected[0] + d[2] * projected[1] + d[4], d[1] * projected[0] + d[3] * projected[1] + d[5]];
        let point = [ox + surface[0] / scale, oy + surface[1] / scale];
        let a = self.area()?;
        if point[0] < a[0] || point[1] < a[1] || point[0] > a[0] + a[2] || point[1] > a[1] + a[3] {
            return fail("Fill point does not intersect the visible drawing");
        }
        Ok(point)
    }
    fn canvas_of_active(&self) -> Outcome<[u32; 2]> {
        let w = self.w()?;
        let tab = ui_session(&w).state().tabs.iter().find(|tab| tab.active).map(|tab| [tab.width, tab.height]);
        tab.map_or_else(|| fail("No active drawing tab"), Ok)
    }

    fn tap_step(&mut self, step: &Value) -> Outcome {
        let tool = step["tool"].as_str().unwrap_or_default().to_owned();
        let document = tool == "fill";
        if document && self.command_enabled("deselect")? {
            self.command("deselect")?;
        }
        self.command(&tool)?;
        let surface = step["surface"].as_str().unwrap_or("canvas").to_owned();
        for point in step["points"].as_array().into_iter().flatten() {
            self.settle("idle before tap")?;
            let before = if document { self.revision()? } else { ui_session(&*self.w()?).state().revision };
            let a = self.area()?;
            let target = [a[0] + a[2] * point[0].as_f64().unwrap_or(0.5), a[1] + a[3] * point[1].as_f64().unwrap_or(0.5)];
            let target = if document { self.fill_point(target)? } else { target };
            let input = self.tap(target, true);
            let ready = self.wait(&format!("{tool} tap"), 120_000, |bench| {
                Ok(if document { bench.revision()? } else { ui_session(&*bench.w()?).state().revision } > before)
            })?;
            self.latency(&tool, input, ready, &surface);
        }
        Ok(())
    }

    fn drag(&mut self, step: &Value) -> Outcome {
        let target = step["target"].as_str().unwrap_or_default().to_owned();
        let duration = self.motion_ms(step);
        if target == "canvas" || target.starts_with("object:") {
            return self.canvas_drag(step, &target, duration);
        }
        self.record["surface"] = json!("window");
        match target.as_str() {
            "slider:size" => self.slider_drag(duration),
            "list:brushes" => self.fling(duration),
            "grip:layers" => self.panel_drag(duration),
            "edge:right-column" => self.edge_drag(duration),
            _ => skip(format!("unknown drag target {target}")),
        }
    }
    fn canvas_drag(&mut self, step: &Value, target: &str, duration: f64) -> Outcome {
        if step["select"] == "all" {
            self.command("select_all")?;
        }
        if let Some(command) = step["command"].as_str() {
            self.command(command)?;
        }
        if let Some(tool) = step["tool"].as_str() {
            self.command(tool)?;
        }
        if let Some(label) = target.strip_prefix("object:") {
            self.select_layer(Some(label))?;
        }
        self.settle("drag target")?;
        let a = self.area()?;
        let start = if target == "canvas" { [a[0] + a[2] * 0.5, a[1] + a[3] * 0.5] } else {
            let w = self.w()?;
            let [width, height] = self.canvas_of_active()?;
            let d = ui_session(&w).state().camera.document_to_surface().map(f64::from);
            let scale = w.window.surface().map_or(1., |s| s.scale());
            let [ox, oy] = Self::origin(&w);
            let (x, y) = (width as f64 / 2., height as f64 / 2.);
            [ox + (d[0] * x + d[2] * y + d[4]) / scale, oy + (d[1] * x + d[3] * y + d[5]) / scale]
        };
        let radius = 0.15 * a[2].min(a[3]);
        let looped = step["path"] == "loop";
        let window = self.contacts(true, duration, 1, |_| 1., |p| {
            let turn = 2. * std::f64::consts::PI * p;
            if looped {
                [a[0] + a[2] * (0.5 + 0.25 * turn.sin()), a[1] + a[3] * (0.5 - 0.25 * turn.cos())]
            } else {
                [start[0] + radius * (1. - (std::f64::consts::PI * p).cos()), start[1] - radius * (std::f64::consts::PI * p).sin()]
            }
        })[0];
        self.motion(window);
        if step["then"] == "apply" {
            self.settle("drag to settle")?;
            let input = now();
            self.command("apply")?;
            let ready = self.settle("apply")?;
            self.latency("apply", input, ready, "canvas");
        }
        Ok(())
    }
    fn touch_drag(&mut self, duration: f64, point: impl Fn(f64) -> [f64; 2]) {
        let window = self.contacts(false, duration, 1, |_| 1., point)[0];
        self.motion(window);
    }
    fn mapped<T: IsA<gtk::Widget>>(root: &impl IsA<gtk::Widget>, accept: impl Fn(&T) -> bool) -> Option<T> {
        widgets(root.as_ref()).filter_map(|widget| widget.downcast::<T>().ok()).find(|widget| widget.is_mapped() && accept(widget))
    }
    fn slider_drag(&mut self, duration: f64) -> Outcome {
        let w = self.w()?;
        let Some(slider) = Self::mapped::<gtk::Scale>(&w.window, |s| s.widget_name() == "component-slider-size") else {
            return skip("no brush size slider is visible in this workspace");
        };
        let [x, y] = Self::widget_center(&w, slider.upcast_ref())?;
        let before = ui_session(&w).state().brush.diameter;
        self.touch_drag(duration, |p| [x + 120. * (std::f64::consts::PI * p).sin(), y]);
        self.settle("slider release")?;
        let after = ui_session(&w).state().brush.diameter;
        self.extra().extend([("size_before".into(), json!(before)), ("size_after".into(), json!(after))]);
        Ok(())
    }
    fn show_panel(&self, panel: &str, visible: bool) -> Outcome {
        self.dispatch(json!({ "type": "customize", "action": { "type": "set_panel_visible", "panel": panel, "visible": visible } }))
    }
    fn panel(name: &str) -> Outcome<Panel> {
        serde_json::from_value(json!(name)).map_err(|_| Stop::Skip(format!("this build has no {name} panel")))
    }
    fn fling(&mut self, duration: f64) -> Outcome {
        let w = self.w()?;
        let panel = Self::panel("brushes")?;
        let shown = w.panel_widget(panel).is_mapped();
        if !shown {
            self.show_panel("brushes", true)?;
            self.settle("brushes panel")?;
        }
        let result = (|| {
            let Some(list) = Self::mapped::<gtk::ScrolledWindow>(&w.panel_widget(panel), |_| true) else {
                return skip("the brushes panel does not scroll in this layout");
            };
            let Some(b) = list.compute_bounds(&w.window) else { return fail("the brushes list has no bounds") };
            let (x, top, height) = ((b.x() + b.width() / 2.) as f64, b.y() as f64, b.height() as f64);
            let begin = now();
            let started = Instant::now();
            self.contacts(false, 120., 1, |_| 1., |p| [x, top + height * (0.75 - 0.5 * p)]);
            pump((duration / 2.) as u64 - (started.elapsed().as_millis() as u64).min((duration / 2.) as u64));
            self.contacts(false, 120., 1, |_| 1., |p| [x, top + height * (0.25 + 0.5 * p)]);
            pump(duration as u64 - (started.elapsed().as_millis() as u64).min(duration as u64));
            self.motion([begin, now()]);
            Ok(())
        })();
        if !shown {
            self.show_panel("brushes", false)?;
        }
        result
    }
    fn placement(&self) -> Outcome<Value> {
        let layout = &self.snapshot()?["workspace"]["layout"];
        Ok(json!([layout["bands"], layout["floating"], layout["collapsed"]]))
    }
    fn panel_drag(&mut self, duration: f64) -> Outcome {
        let w = self.w()?;
        let layers = Self::panel("layers")?;
        let resolved = w.resolved();
        let Some(group) = resolved.groups.iter().find(|g| g.panels.contains(&layers) && !g.floating) else {
            return skip("the Layers panel is not docked in this workspace");
        };
        let b = group.bounds;
        let start = (0..24).flat_map(|i| (0..4).map(move |j| [b.x + b.width * (i as f32 + 0.5) / 24., b.y + 4. + 8. * j as f32]))
            .find(|p| w.drag_target_at(*p).is_some());
        let Some(start) = start else { return skip("the Layers group has no drag handle in its header") };
        let [ox, oy] = Self::origin(&w);
        let start = [ox + start[0] as f64, oy + start[1] as f64];
        let before = self.placement()?;
        let a = self.area()?;
        let float = [a[0] + a[2] * 0.5, a[1] + a[3] * 0.5];
        let radius = 0.1 * a[2].min(a[3]);
        let lerp = |from: [f64; 2], to: [f64; 2], t: f64| [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t];
        self.touch_drag(duration, |p| {
            if p < 0.25 { lerp(start, float, p / 0.25) }
            else if p < 0.75 {
                let q = 2. * std::f64::consts::PI * (p - 0.25) / 0.5;
                [float[0] + radius * q.sin(), float[1] - radius * (1. - q.cos())]
            } else { lerp(float, start, (p - 0.75) / 0.25) }
        });
        self.settle("panel drop")?;
        let restored = self.placement()? == before;
        self.extra().insert("returned_in_place".into(), json!(restored));
        if !restored {
            self.command("undo_workspace")?;
            self.settle("workspace undo")?;
            return fail("Layers did not return to its starting dock");
        }
        Ok(())
    }
    fn edge_drag(&mut self, duration: f64) -> Outcome {
        let w = self.w()?;
        let resolved = w.resolved();
        let middle = resolved.work_area.x + resolved.work_area.width / 2.;
        let Some(divider) = resolved.dividers.iter().filter(|d| d.band && !d.fixed && d.bounds.height > d.bounds.width && d.bounds.x > middle)
            .min_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x)) else {
            return skip("no resizable column edge right of the canvas in this workspace");
        };
        let [ox, oy] = Self::origin(&w);
        let start = [ox + (divider.bounds.x + divider.bounds.width / 2.) as f64, oy + (divider.bounds.y + divider.bounds.height / 2.) as f64];
        let id = divider.id;
        self.extra().insert("divider".into(), json!(id));
        self.touch_drag(duration, |p| [start[0] - 120. * (std::f64::consts::PI * p).sin(), start[1]]);
        Ok(())
    }

    fn scrub(&mut self, step: &Value) -> Outcome {
        self.record["input"] = json!("dispatch");
        let frames = step["frames"].as_u64().or(self.chain["motion_frames"].as_u64()).unwrap_or(150) as usize;
        if let Some(effect) = step["effect"].as_str() {
            if effect != "curves" {
                return skip(format!("no scrub value is defined for {effect}"));
            }
            self.select_layer(Some("Photo"))?;
            self.dispatch(json!({ "type": "effect", "action": { "op": "insert", "effect": effect } }))?;
            self.settle("curves layer")?;
            let Some(layer) = self.editing_layer()? else { return fail("Curves did not select its layer") };
            self.extra().insert("effect_layer".into(), json!(layer));
            let result = self.per_frame(frames, move |i| {
                let y = 0.5 + 0.25 * (2. * std::f64::consts::PI * i as f64 / frames as f64).sin();
                let phase = if i == 0 { "down" } else if i + 1 == frames { "up" } else { "move" };
                json!({ "type": "effect", "action": { "op": "gesture", "phase": phase, "action": {
                    "op": "set", "layer": layer, "key": "rgb", "value": { "kind": "curve", "value": [[0, 0], [0.5, y], [1, 1]] } } } })
            }).and_then(|window| {
                self.motion(window);
                self.settle("curves to settle").map(|_| ())
            });
            self.dispatch(json!({ "type": "layer", "action": { "op": "delete", "id": layer } }))?;
            return result;
        }
        let ramp: Vec<f64> = step["layer_opacity"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
        if ramp.len() != 2 {
            return skip("no layer opacity ramp");
        }
        self.select_layer(None)?;
        let window = self.per_frame(frames, move |i| {
            json!({ "type": "set_layer_opacity", "opacity": ramp[0] + (ramp[1] - ramp[0]) * i as f64 / (frames.max(2) - 1) as f64 })
        })?;
        self.motion(window);
        self.settle("opacity to settle")?;
        Ok(())
    }

    fn repeat(&mut self, step: &Value) -> Outcome {
        let surface = step["surface"].as_str().unwrap_or("canvas").to_owned();
        let count = step["count"].as_u64().unwrap_or(1);
        let names = |key: &str| step[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>();
        for _ in 0..count {
            if step.get("commands").is_some() {
                self.record["input"] = json!("dispatch");
                for name in names("commands") {
                    let input = now();
                    self.command(&name)?;
                    let ready = self.settle(&name)?;
                    self.latency(&name, input, ready, &surface);
                }
            } else if step.get("open").is_some() {
                for spec in names("open") {
                    self.open_item(&spec, &surface)?;
                }
            } else if step.get("workspaces").is_some() {
                self.record["input"] = json!("dispatch");
                for name in names("workspaces") {
                    let id = match name.as_str() {
                        "start" => self.start_workspace.clone().map_or_else(|| skip("the start workspace is unknown"), Ok)?,
                        "photo" => PHOTO_WORKSPACE.to_owned(),
                        other => format!("builtin:workspace:{other}"),
                    };
                    if self.start_workspace.as_deref() == Some(PHOTO_WORKSPACE) {
                        return skip("Photo is already the starting workspace, so there is no other workspace to return to");
                    }
                    let switch: layer_workspace::WorkspaceInput = serde_json::from_value(json!({ "type": "switch", "id": id }))
                        .map_err(|_| Stop::Skip("this build cannot switch workspaces by id".into()))?;
                    let w = self.w()?;
                    let input = now();
                    w.workspaces.send(&w, switch);
                    let ready = self.wait(&format!("workspace {name}"), 120_000, |bench| {
                        let view = bench.w()?.workspaces.view();
                        Ok(view.id.as_deref() == Some(id.as_str()) && !view.busy && !view.loading && Self::quiet(&bench.w()?))
                    })?;
                    self.latency(&name, input, ready, &surface);
                }
            } else if step.get("tabs").is_some() {
                for name in names("tabs") {
                    let Some(tab) = self.documents.get(&name).map(|opened| opened.tab) else { return skip(format!("{name} is not open")) };
                    let w = self.w()?;
                    let widget = find_named(w.window.upcast_ref(), &format!("document-tab-{tab}")).filter(|widget| widget.is_mapped());
                    let input = match widget {
                        Some(widget) => {
                            let center = Self::widget_center(&w, &widget)?;
                            self.tap(center, false)
                        }
                        None => {
                            self.record["input"] = json!("dispatch");
                            let input = now();
                            w.documents.select(&w, tab, false);
                            input
                        }
                    };
                    let ready = self.wait(&format!("tab {name}"), 120_000, |bench| {
                        let w = bench.w()?;
                        Ok(w.documents.selected() == tab && Self::quiet(&w))
                    })?;
                    self.latency(&format!("tab:{name}"), input, ready, &surface);
                }
            } else {
                return skip("nothing to repeat");
            }
        }
        Ok(())
    }
    fn open_item(&mut self, spec: &str, surface: &str) -> Outcome {
        let w = self.w()?;
        if spec == "menu:main" {
            let menus: Vec<gtk::MenuButton> = ["Primary", "File"].iter()
                .filter_map(|id| find_named(w.window.upcast_ref(), &format!("application-menu-{id}")))
                .filter(|widget| widget.is_mapped()).filter_map(|widget| widget.downcast().ok()).collect();
            let Some(menu) = menus.into_iter().next() else { return skip("no main menu control is visible") };
            let center = Self::widget_center(&w, menu.upcast_ref())?;
            let open = |menu: &gtk::MenuButton| menu.popover().is_some_and(|p| p.is_mapped());
            let input = self.tap(center, false);
            let ready = self.wait("main menu", 30_000, |_| Ok(open(&menu)))?;
            self.latency(spec, input, ready, surface);
            self.key(0xff1b);
            return self.wait("main menu to close", 30_000, |_| Ok(!open(&menu))).map(|_| ());
        }
        let name = spec.strip_prefix("panel:").unwrap_or(spec);
        let panel = Self::panel(name)?;
        let docked = w.panel_widget(panel).is_mapped();
        if docked {
            self.show_panel(name, false)?;
            self.wait(&format!("{name} to close"), 30_000, |_| Ok(!w.panel_widget(panel).is_mapped()))?;
        }
        self.record["input"] = json!("dispatch");
        let input = now();
        self.show_panel(name, true)?;
        let layout = ui_session(&w).state().workspace.layout.clone();
        if layout.active_panel(panel) != Some(panel) && let Some(group) = layout.panel_group(panel) {
            w.dispatch(UiAction::SelectPanelTab { group, panel });
        }
        let ready = self.wait(&format!("{name} to open"), 30_000, |_| Ok(w.panel_widget(panel).is_mapped()))?;
        self.latency(spec, input, ready, surface);
        if !docked {
            self.show_panel(name, false)?;
            self.wait(&format!("{name} to close"), 30_000, |_| Ok(!w.panel_widget(panel).is_mapped()))?;
        }
        Ok(())
    }

    fn save(&mut self, name: &str) -> Outcome {
        self.record["input"] = json!("dispatch");
        self.record["surface"] = json!("window");
        let Some(tab) = self.documents.get(name).map(|opened| opened.tab) else { return skip(format!("{name} is not open")) };
        let w = self.w()?;
        if w.documents.selected() != tab {
            w.documents.select(&w, tab, false);
            self.wait(&format!("tab {name}"), 120_000, |bench| Ok(bench.w()?.documents.selected() == tab))?;
        }
        let input = now();
        self.command("save_document")?;
        let ready = self.wait("save", 120_000, |bench| {
            let w = bench.w()?;
            if let Some(dialog) = Self::prompt(&w) {
                return fail(format!("Saving showed the {dialog} dialog"));
            }
            let file = ui_session(&w).state().document_file.clone();
            Ok(!file.modified && !file.busy)
        })?;
        self.latency("save", input, ready, "window");
        Ok(())
    }

    fn suspended(w: &Workspace) -> bool {
        w.window.surface().and_downcast::<gdk::Toplevel>().is_some_and(|t| t.state().contains(gdk::ToplevelState::SUSPENDED))
    }
    fn resume(&mut self) -> Outcome {
        self.record["input"] = json!("dispatch");
        let w = self.w()?;
        w.window.minimize();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !Self::suspended(&w) {
            if Instant::now() > deadline { return skip("the compositor did not suspend the minimized window"); }
            pump(5);
        }
        pump(1000);
        let input = now();
        w.window.present();
        let ready = self.wait("resume", 60_000, |bench| {
            let w = bench.w()?;
            Ok(!Self::suspended(&w) && Self::quiet(&w))
        })?;
        self.latency("resume", input, ready, "canvas");
        Ok(())
    }

    fn close(&mut self) -> Outcome {
        self.record["input"] = json!("dispatch");
        self.record["surface"] = json!("window");
        while let Some(w) = self.w.clone().filter(|w| w.window.is_visible()) {
            let count = w.documents.len();
            let input = now();
            w.documents.select(&w, w.documents.selected(), true);
            let deadline = Instant::now() + Duration::from_secs(120);
            loop {
                if Self::prompt(&w).is_some() {
                    super::new_photo::response(&w, "discard");
                }
                if !w.window.is_visible() || w.documents.len() < count && !w.documents.changing.get() { break; }
                if Instant::now() > deadline { return fail("The drawing did not close"); }
                pump(2);
            }
            if !w.window.is_visible() {
                self.latency("close", input, now(), "window");
                self.w = None;
            } else {
                let ready = self.settle("close")?;
                self.latency("close", input, ready, "window");
            }
        }
        Ok(())
    }

    fn perform_step(&mut self, step: &Value) -> Outcome {
        match step["do"].as_str().unwrap_or_default() {
            "launch" => self.launch(),
            "open" => self.open(step["document"].as_str().unwrap_or_default()),
            "stroke" => self.stroke(step),
            "gesture" => self.gesture(step),
            "drag" => self.drag(step),
            "tap" => self.tap_step(step),
            "repeat" => self.repeat(step),
            "scrub" => self.scrub(step),
            "save" => self.save(step["document"].as_str().unwrap_or_default()),
            "resume" => self.resume(),
            "close" => self.close(),
            other => skip(format!("unknown step kind {other}")),
        }
    }

    fn canvas_frames(&self) -> Vec<Value> {
        let mut frames: Vec<Value> = self.renderers.iter().flat_map(|stats| {
            let stats = stats.lock().unwrap();
            let queued: HashMap<u64, u64> = stats.cpu.iter().map(|row| (row[0] as u64, row[4] as u64)).collect();
            stats.presented.iter().filter(|p| p[3] == 1 && p[1] > 0)
                .map(|p| json!([p[1], queued.get(&p[0]).copied()])).collect::<Vec<_>>()
        }).collect();
        frames.sort_by_key(|row| row[0].as_u64());
        frames
    }
}

#[test]
#[ignore = "bench_gtk.py: private Wayland display, hardware GPU, --tablet, --native-storage and --native-recovery"]
fn native_bench_chain() {
    assert_ne!(glib::user_data_dir(), glib::home_dir().join(".local/share"), "the bench chain needs a private XDG_DATA_HOME");
    let storage = std::path::PathBuf::from(std::env::var_os("CAPY_STORAGE_DIR").expect("a private CAPY_STORAGE_DIR"));
    if let Some(shaders) = std::env::var_os("CAPY_BENCH_SHADERS") {
        std::fs::create_dir_all(storage.join("cache")).unwrap();
        std::os::unix::fs::symlink(shaders, storage.join("cache/shaders")).unwrap();
    }
    let output = std::path::PathBuf::from(std::env::var_os("CAPY_BENCH_OUTPUT").expect("CAPY_BENCH_OUTPUT"));
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("pid"), std::process::id().to_string()).unwrap();
    let chain: Value = serde_json::from_slice(&std::fs::read(std::env::var_os("CAPY_BENCH_CHAIN").expect("CAPY_BENCH_CHAIN")).unwrap()).unwrap();
    assert_eq!(chain["schema"], 1, "unknown chain schema");
    let selected: Option<Vec<String>> = std::env::var("CAPY_BENCH_STEPS").ok().filter(|s| !s.is_empty())
        .map(|steps| steps.split(',').map(str::to_owned).collect());
    let files = chain["documents"].as_object().unwrap().keys()
        .filter_map(|name| std::env::var_os(format!("CAPY_BENCH_{}", name.to_uppercase())).map(|path| (name.clone(), path.into())))
        .collect();
    let event_ms = std::env::var("LAYER_NATIVE_EVENT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(4.);
    let input = RemoteInput::new().settle_ms(0).timeout_secs(120);
    input.ready();
    let mut bench = Bench {
        chain: chain.clone(), files, input, event_ms, app: None, w: None, record: Value::Null, refresh_hz: 60., scale: None,
        canvas: None, start_workspace: None, documents: HashMap::new(), shown: None, renderers: Vec::new(), window_frames: Rc::default(),
    };
    let mut steps = Vec::new();
    let mut stuck: Option<String> = None;
    let mut opened: Option<String> = None;
    let write = |bench: &Bench, steps: &[Value]| {
        let journey = json!({
            "pid": std::process::id(), "refresh_hz": bench.refresh_hz, "canvas": bench.canvas, "event_ms": bench.event_ms,
            "scale": bench.scale,
            "steps": steps, "canvas_frames": bench.canvas_frames(),
            "window_frames": bench.window_frames.borrow().values().collect::<Vec<_>>(),
        });
        let partial = output.join("journey.partial");
        std::fs::write(&partial, serde_json::to_vec(&journey).unwrap()).unwrap();
        std::fs::rename(partial, output.join("journey.json")).unwrap();
    };
    for step in chain["steps"].as_array().unwrap() {
        let id = step["id"].as_str().unwrap().to_owned();
        if selected.as_ref().is_some_and(|s| !s.contains(&id)) { continue; }
        bench.record = json!({
            "id": id, "status": "ok", "reason": null, "input": "real", "begin_ns": now(), "end_ns": null,
            "surface": step["surface"].as_str().unwrap_or("canvas"), "motion": [], "contacts": [], "completions": [],
            "settled_ns": null, "latency": [], "extra": {},
        });
        let kind = step["do"].as_str().unwrap_or_default();
        if kind == "open" {
            opened = step["document"].as_str().map(str::to_owned);
        }
        let needs: Vec<String> = opened.iter().filter(|_| !matches!(kind, "launch" | "open" | "resume" | "close")).cloned()
            .chain(step["document"].as_str().map(str::to_owned).filter(|_| kind != "open"))
            .chain(step["tabs"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned)).collect();
        let missing = needs.into_iter().find(|name| !bench.documents.contains_key(name));
        let outcome = if let Some(reason) = &stuck {
            Err(Stop::Fail(format!("not run: {reason}")))
        } else if let Some(name) = missing {
            Err(Stop::Skip(format!("{name} did not open")))
        } else {
            catch_unwind(AssertUnwindSafe(|| {
                if kind != "launch" {
                    bench.w()?;
                    bench.settle(&format!("idle before {id}"))?;
                }
                bench.record["begin_ns"] = json!(now());
                bench.perform_step(step)
            })).unwrap_or_else(|panic| Err(Stop::Fail(panic.downcast_ref::<String>().cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into()))))
        };
        match outcome {
            Ok(()) => {}
            Err(Stop::Skip(reason)) => { bench.record["status"] = json!("skipped"); bench.record["reason"] = json!(reason); }
            Err(Stop::Fail(reason)) => {
                if reason.starts_with("Timed out waiting for idle") || kind == "launch" { stuck = Some(reason.clone()); }
                bench.record["status"] = json!("error");
                bench.record["reason"] = json!(reason);
            }
        }
        bench.record["end_ns"] = json!(now());
        bench.collect_renderer();
        println!("BENCH_STEP {id} {} {}", bench.record["status"].as_str().unwrap(), bench.record["reason"].as_str().unwrap_or(""));
        steps.push(std::mem::take(&mut bench.record));
        write(&bench, &steps);
    }
    if let Some(w) = bench.w.take() {
        w.window.destroy();
    }
    write(&bench, &steps);
    bench.input.finish();
    drop(bench.app.take());
}
