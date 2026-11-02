use super::workspace_header::Driver;
use super::*;

struct Samples {
    clock: gdk::FrameClock,
    handler: glib::SignalHandlerId,
    frames: Rc<RefCell<Vec<(bool, gdk::FrameTimings)>>>,
    started: i64,
}
impl Samples {
    fn start(w: &Rc<Workspace>, key: impl Fn() -> Option<[i64; 4]> + 'static) -> Self {
        let clock = w.surface.frame_clock().unwrap();
        let previous = Cell::new(key());
        let frames = Rc::new(RefCell::new(Vec::with_capacity(1200)));
        let handler = clock.connect_after_paint(glib::clone!(
            #[strong]
            frames,
            move |clock| {
                let next = key();
                let changed = previous.replace(next) != next;
                if let Some(timing) = clock.current_timings() {
                    frames.borrow_mut().push((changed, timing));
                }
            }
        ));
        Self { clock, handler, frames, started: glib::monotonic_time() }
    }
    fn finish(self) -> serde_json::Value {
        let ended = glib::monotonic_time();
        self.clock.disconnect(self.handler);
        pump(100);
        let frames = self.frames.borrow();
        let times = |moving: bool| {
            let mut values: Vec<_> = frames
                .iter()
                .filter(|(changed, timing)| {
                    (!moving || *changed) && timing.is_complete() && timing.presentation_time() > 0
                })
                .map(|(_, timing)| timing.presentation_time())
                .collect();
            values.sort_unstable();
            values.dedup();
            values
        };
        let all = times(false);
        let moving = times(true);
        let mut gaps: Vec<_> = moving.windows(2).map(|pair| (pair[1] - pair[0]) as f64 / 1000.).collect();
        gaps.sort_by(f64::total_cmp);
        let seconds = moving.first().zip(moving.last()).map(|(a, b)| (b - a) as f64 / 1e6);
        serde_json::json!({
            "contact_duration_s": (ended - self.started) as f64 / 1e6,
            "full_contact_presentations": all.len(),
            "full_contact_longest_gap_ms": all.windows(2).map(|pair| (pair[1] - pair[0]) as f64 / 1000.).max_by(f64::total_cmp),
            "moving_presentations": moving,
            "moving_seconds": seconds,
            "presentation_hz": seconds.filter(|s| *s > 0.).map(|s| gaps.len() as f64 / s),
            "gap_p99_ms": gaps.get(gaps.len() * 99 / 100),
            "longest_gap_ms": gaps.last(),
            "onset_ms": moving.first().map(|time| (time - self.started) as f64 / 1000.),
            "tail_ms": moving.last().map(|time| (ended - time) as f64 / 1000.),
        })
    }
}

fn bounds(widget: &gtk::Widget, w: &Workspace) -> Option<[f32; 4]> {
    if !widget.is_mapped() {
        return None;
    }
    let rect = widget.compute_bounds(&w.surface)?;
    let result = [rect.x(), rect.y(), rect.width(), rect.height()];
    (rect.width() > 0.
        && rect.height() > 0.
        && rect.x() >= 0.
        && rect.y() >= 0.
        && rect.x() + rect.width() <= w.surface.width() as f32 + 1.
        && rect.y() + rect.height() <= w.surface.height() as f32 + 1.)
        .then_some(result)
}

fn opener(d: &Driver, control: ToolbarControl, phone: bool) -> Option<gtk::Widget> {
    let layout = state(&d.w).workspace.layout;
    let name = if phone {
        let value = serde_json::to_value(&layout).unwrap();
        let control = serde_json::to_value(control).unwrap();
        let tile = value.get("phone")?.as_array()?.iter().find(|tile| tile["control"] == control)?;
        format!("phone-tool-{}", tile["id"].as_u64()?)
    } else {
        let entry = layout.header.entries().find(|entry| entry.item == HeaderItem::Tool { control })?;
        format!("header-item-{}", entry.id)
    };
    find_named(d.w.window.upcast_ref(), &name).filter(|widget| bounds(widget, &d.w).is_some())
}

fn slider(d: &Driver, control: ToolbarControl, phone: bool) -> Option<gtk::Scale> {
    let name = if phone {
        let setting = if control == ToolbarControl::BrushSizeSlider { "size" } else { "strength" };
        format!("component-slider-{setting}")
    } else {
        let id = state(&d.w)
            .workspace
            .layout
            .panels
            .iter()
            .flat_map(|panel| panel.tiles())
            .find(|tile| tile.control == control)?
            .id;
        format!("component-slider-{id}")
    };
    find_named(d.w.window.upcast_ref(), &name)?
        .downcast::<gtk::Scale>()
        .ok()
        .filter(|widget| bounds(widget.upcast_ref(), &d.w).is_some())
}

pub(super) fn setup(name: &str) -> Driver {
    assert!(crate::storage::workspaces().is_some());
    let app = native_test_app(name);
    let settings = gtk::Settings::default().unwrap();
    if let Ok(layout) = std::env::var("LAYER_PHONE_DECORATION_LAYOUT") {
        settings.set_gtk_decoration_layout(Some(&layout));
    }
    if let Ok(size) = std::env::var("LAYER_PHONE_TEXT_PT") {
        let size = size.parse::<f32>().unwrap();
        assert!(size.is_finite() && size > 0.);
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&format!("window.capy-workspace {{ --ui-text-size: {size}pt; }}"));
        gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    }
    let w = Workspace::with_project(&app, Some((new_drawing_at(1200, 1600, layer_core::color::SampleDepth::U8), None)));
    apply_fixture_theme(&w);
    w.window.maximize();
    w.window.present();
    new_photo::ready(&w);
    w.workspaces.send(&w, layer_workspace::WorkspaceInput::Switch { id: "builtin:workspace:painter".into() });
    wait_workspaces(&w);
    new_photo::ready(&w);
    assert_eq!(w.workspaces.view().id.as_deref(), Some("builtin:workspace:painter"));
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(300);
    Driver::start(w, app)
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_motion --native-storage; LAYER_NATIVE_EVENT_MS=4, viewport/theme external"]
fn native_phone_motion() {
    assert_eq!(std::env::var("LAYER_NATIVE_EVENT_MS").unwrap(), "4");
    let mut d = setup("art.capycanvas.PhoneMotion");
    d.input.settle_ms = 0;
    d.input.timeout = Duration::from_secs(30);
    let layout = serde_json::to_value(&state(&d.w).workspace.layout).unwrap();
    let phone = layout.get("phone").is_some_and(|tiles| tiles.is_array());
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    let mut available = Vec::new();
    let mut reports = Vec::new();
    for (name, control) in
        [("color", ToolbarControl::Color), ("layers", ToolbarControl::Panel { panel: Panel::Layers })]
    {
        let Some(button) = opener(&d, control, phone) else {
            available.push(serde_json::json!({"control": name, "available": false}));
            continue;
        };
        available.push(serde_json::json!({"control": name, "available": true, "bounds": bounds(&button, &d.w)}));
        for _ in 0..2 {
            d.click(&button);
            pump(350);
        }
        assert!(state(&d.w).customization.drawer.is_none());
        for run in 0..3 {
            let began = Instant::now();
            let mut transitions = 0;
            while began.elapsed() < Duration::from_secs(6) {
                for opening in [true, false] {
                    let w = d.w.clone();
                    let scale = crate::squircle::device_scale(&w.window) as f32;
                    let samples = Samples::start(&d.w, move || {
                        w.drawer.placement().map(|place| {
                            let b = place.bounds;
                            [b.x, b.y, b.width, b.height].map(|v| (v * scale).round() as i64)
                        })
                    });
                    d.click(&button);
                    pump(300);
                    assert_eq!(state(&d.w).customization.drawer.is_some(), opening);
                    let mut report = samples.finish();
                    assert!(
                        report["moving_presentations"].as_array().unwrap().len() > 2,
                        "{name}: animate actual drawer geometry"
                    );
                    report["control"] = name.into();
                    report["opening"] = opening.into();
                    report["run"] = run.into();
                    reports.push(report);
                    transitions += 1;
                }
            }
            assert!(transitions >= 6);
        }
    }
    for (name, control) in
        [("size", ToolbarControl::BrushSizeSlider), ("strength", ToolbarControl::BrushStrengthSlider)]
    {
        let Some(track) = slider(&d, control, phone) else {
            available.push(serde_json::json!({"control": name, "available": false}));
            continue;
        };
        available.push(serde_json::json!({"control": name, "available": true, "bounds": bounds(track.upcast_ref(), &d.w)}));
        let window = d.w.window.clone();
        let point = |fraction| {
            screen_point(
                track.upcast_ref(),
                &window,
                if track.orientation() == gtk::Orientation::Horizontal { [fraction, 0.5] } else { [0.5, fraction] },
            )
        };
        d.input.perform(serde_json::json!([
            contact("mouse", "down", point(0.2)),
            contact("mouse", "move", point(0.8)),
            contact("mouse", "up", point(0.8))
        ]));
        pump(150);
        for run in 0..3 {
            let tracked = track.clone();
            let scale = crate::squircle::device_scale(&d.w.window);
            let samples = Samples::start(&d.w, move || {
                let (start, end) = tracked.slider_range();
                Some([(start as f64 * scale).round() as i64, (end as f64 * scale).round() as i64, 0, 0])
            });
            let mut events = vec![contact("mouse", "down", point(0.2))];
            events.extend((0..1500).map(|step| {
                let phase = (step as f32 / 1500. * 16.) % 2.;
                contact("mouse", "move", point(if phase < 1. { 0.2 + 0.6 * phase } else { 0.8 - 0.6 * (phase - 1.) }))
            }));
            events.push(contact("mouse", "up", point(0.2)));
            d.input.perform(serde_json::Value::Array(events));
            let mut report = samples.finish();
            assert!(
                report["moving_presentations"].as_array().unwrap().len() > 20,
                "{name}: sustained native slider movement"
            );
            assert!(report["contact_duration_s"].as_f64().unwrap() >= 5.);
            report["control"] = name.into();
            report["run"] = run.into();
            report["planned_moves"] = 1500.into();
            reports.push(report);
        }
    }
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    assert!(!reports.is_empty(), "at least one existing control must be measurable");
    let report = serde_json::json!({
        "presentation": if phone {"phone"} else {"baseline_sketch"},
        "viewport": [d.w.surface.width(), d.w.surface.height()],
        "window": [d.w.window.width(), d.w.window.height()],
        "theme": std::env::var("CAPY_NATIVE_TEST_THEME").unwrap(),
        "drawing": [1200, 1600],
        "work_area": state(&d.w).camera.work_area,
        "available_controls": available,
        "measurements": reports,
    });
    let path = std::path::PathBuf::from(std::env::var_os("LAYER_PHONE_MOTION_REPORT").unwrap());
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    if phone {
        assert!(
            report["available_controls"].as_array().unwrap().iter().all(|control| control["available"] == true),
            "all four phone controls must be measurable"
        );
    }
    d.finish();
}
