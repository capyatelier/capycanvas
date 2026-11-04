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

pub(super) fn export_allocations(w: &Workspace, state_name: &str, capture_name: &str) {
    let Some(directory) = std::env::var_os("LAYER_NATIVE_CAPTURE_DIR") else { return; };
    let state = state(w);
    let (document,camera) = {
        let session = ui_session(w);
        let document = session.engine().document();
        let composition = document.composition();
        let scene = document.scene();
        let paper = document.bottom_paper().map(|occurrence| {
            let color = scene.effect(occurrence).and_then(|effect| match effect.value("color") {
                Some(layer_core::EffectValue::Color(color)) => Some(*color), _ => None,
            });
            let relief = scene.effect(occurrence).and_then(layer_core::paper::light).map(|light| light.relief);
            serde_json::json!({"visible":scene.visible(occurrence),"color":color,"relief":relief})
        });
        (serde_json::json!({"extent":composition.size,"color":composition.color,"paper":paper}),
            serde_json::to_value(session.camera()).unwrap())
    };
    let viewport = [w.window.width(), w.window.height()];
    let dpr = w.window.scale_factor();
    assert_eq!(dpr,1,"the phone audit capture matrix requires actual DPR1");
    let phone = state.phone_presentation([w.surface.width() as f32, w.surface.height() as f32]);
    let all = widgets(w.window.upcast_ref()).collect::<Vec<_>>();
    let named = |name: &str| {
        let matching = all.iter().filter(|widget| widget.widget_name() == name).cloned().collect::<Vec<_>>();
        assert!(matching.iter().filter(|widget| widget.is_mapped()).count() <= 1,
            "allocation export has duplicate mapped widget {name}");
        matching.iter().find(|widget| widget.is_mapped()).or_else(|| matching.first()).cloned()
    };
    let mut elements = Vec::new();
    let mut missing = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut widget_sources = serde_json::Map::new();
    let mut add = |id: &str, role: &str, widget: Option<gtk::Widget>| {
        assert!(ids.insert(id.to_owned()), "duplicate allocation semantic ID {id}");
        let Some(widget) = widget else { missing.push(id.to_owned()); return; };
        let visible = widget.is_mapped();
        let bounds = visible.then(|| widget.compute_bounds(&w.window)).flatten()
            .map(|rect| [rect.x(), rect.y(), rect.width(), rect.height()]);
        if visible && bounds.is_none() { missing.push(id.to_owned()); }
        elements.push(serde_json::json!({"id":id,"role":role,"bounds":bounds,"visible":visible}));
        widget_sources.insert(id.into(), serde_json::json!({"widget_name":widget.widget_name().to_string(),"type":widget.type_().name().to_string()}));
    };
    if phone.is_some() {
        for (id, role, name) in [
            ("bar.tools","bar","phone-bar-tools"),("bar.capy","bar","phone-bar-capy"),
            ("chip.tools","chip","phone-tools-chip"),("chip.capy","chip","phone-sliders-chip"),
            ("capy","tile","zen-capy"),("tile.settings","tile","phone-tool-settings"),
            ("native.start","native-reservation","native-group-0"),("native.end","native-reservation","native-group-1"),
        ] { add(id,role,named(name)); }
        if let Some(title) = named("phone-title-band") { add("title","window-move",Some(title)); }
        for tile in state.workspace.layout.phone.iter().flatten().filter(|tile| tile.control != ToolbarControl::Divider) {
            add(&format!("tile.{}",tile.id),"tile",named(&format!("phone-tool-{}",tile.id)));
        }
        for name in ["size","strength"] {
            let slider = named(&format!("component-slider-{name}"));
            add(&format!("slider.{name}"),"slider",slider.clone());
            if let Some(slider) = slider {
                for part in ["trough","slider"] {
                    let widget = widgets(&slider).find(|widget| widget.css_name() == part);
                    add(&format!("slider.{name}.{}",if part == "trough" { "track" } else { "handle" }),part,widget);
                }
            }
            if let Some(value) = named(&format!("slider-value-{name}")) { add(&format!("slider.{name}.value"),"value",Some(value)); }
        }
        if let Some(preview) = named("brush-slider-preview").filter(|widget| widget.is_mapped()) {
            add("slider.preview","preview",Some(preview));
        }
    }
    if let Some(drawer) = state.customization.drawer.as_ref() {
        add("drawer","drawer",named("tool-drawer"));
        add("drawer.bridge","bridge",named("drawer-connection"));
        let drawer_type = named("tool-drawer").map(|widget| widget.type_());
        for (index,column) in drawer.columns.iter().enumerate() {
            let widget = column.first().and_then(|panel| named(&format!("drawer-panel-{panel:?}")))
                .and_then(|widget| std::iter::successors(Some(widget), |widget| widget.parent())
                    .find(|widget| widget.parent().is_some_and(|parent| Some(parent.type_()) == drawer_type)));
            add(&format!("drawer.column.{index}"),"drawer-column",widget);
        }
        for panel in drawer.panels() {
            let body = named(&format!("drawer-panel-{panel:?}"));
            add(&format!("drawer.panel.{panel:?}"),"drawer-panel",body.clone());
            if let Some(body) = body {
                if panel == Panel::Palettes {
                    let overlay = body.first_child().and_downcast::<gtk::Overlay>().unwrap();
                    let normal = overlay.child().unwrap();
                    let history = normal.first_child().unwrap();
                    let scroll = history.next_sibling().unwrap().next_sibling().unwrap();
                    for (id, role, widget) in [("body", "panel-body", overlay.upcast()), ("history", "grid", history), ("scroll", "scroll", scroll)] {
                        add(&format!("drawer.palettes.{id}"),role,Some(widget));
                    }
                    for (id, role, name) in [("footer", "toolbar", "palette-footer"), ("name", "button", "palette-color-name"), ("detail", "label", "palette-color-detail")] {
                        let widget = if id == "footer" { widgets(&body).find(|widget| widget.has_css_class(name)) } else { named(name) };
                        add(&format!("drawer.palettes.{id}"),role,widget);
                    }
                }
                let panel_value = serde_json::to_value(panel).unwrap();
                let Some(panel_id) = panel_value.as_str() else { continue; };
                for (kind, class) in [("groups", "tool-group"), ("subtools", "brush-choice")] {
                    for (index, button) in widgets(&body).filter(|widget| widget.has_css_class(class)).enumerate() {
                        let id = format!("drawer.{panel_id}.{kind}.{index}");
                        add(&id,"row",Some(button.clone()));
                        add(&format!("{id}.icon"),"icon",widgets(&button).find(|widget| widget.is::<gtk::Image>()));
                        add(&format!("{id}.label"),"label",widgets(&button).find(|widget| widget.is::<gtk::Label>()));
                        if let Some(preview) = widgets(&button).find(|widget| widget.is::<gtk::Picture>()) {
                            add(&format!("{id}.preview"),"preview",Some(preview));
                        }
                    }
                }
            }
        }
        if drawer.panels().any(|panel| panel == Panel::Color) { add("drawer.color.wheel","color-wheel",named("color-wheel")); }
    }
    if state.phone.menu {
        let bridge = all.iter().find(|widget| widget.is_mapped() && widget.widget_name() == "drawer-connection").cloned();
        add("menu.bridge","bridge",bridge);
        for (id,role,name) in [("menu","menu","phone-menu"),("menu.list","menu-list","phone-menu-scroll"),
            ("menu.separator","separator","phone-menu-separator")] {
            add(id,role,named(name));
        }
        let page = named("phone-menu-pages").and_downcast::<gtk::Stack>().and_then(|stack| stack.visible_child());
        add("menu.page","menu-page",page);
        for name in ["preferences","search","finger","zen","swap"] {
            add(&format!("menu.tile.{name}"),"menu-tile",named(&format!("phone-{name}")));
        }
        if let Some(back) = named("phone-menu-back").filter(|widget| widget.is_mapped()) { add("menu.back","back",Some(back)); }
        if let Some(list) = named("phone-menu-list") {
            for (index,row) in widgets(&list).filter(|widget| widget.is_mapped() && widget.css_name() == "modelbutton").enumerate() {
                let children = widgets(&row).collect::<Vec<_>>();
                add(&format!("menu.row.{index}"),"menu-row",Some(row.clone()));
                add(&format!("menu.row.{index}.label"),"menu-label",children.iter().find(|widget| widget.is::<gtk::Label>() && widget.widget_name() != "menu-detail").cloned());
                for (part,role,child) in [
                    ("icon", "menu-icon", children.iter().find(|widget| widget.is::<gtk::Image>() && widget.is_visible() && !widget.has_css_class("menu-check") && !widget.has_css_class("menu-arrow")).cloned()),
                    ("check", "menu-check", children.iter().find(|widget| widget.has_css_class("menu-check")).cloned()),
                    ("arrow", "menu-arrow", children.iter().find(|widget| widget.has_css_class("menu-arrow")).cloned()),
                    ("detail", "menu-detail", children.iter().find(|widget| widget.widget_name() == "menu-detail" && widget.is_visible()).cloned()),
                    ("close", "menu-close", row.next_sibling().filter(|widget| widget.has_css_class("header-tool"))),
                ] { if let Some(child) = child { add(&format!("menu.row.{index}.{part}"),role,Some(child)); } }

            }
        }
    }
    for panel in all.iter().filter(|widget| widget.is_mapped() && widget.has_css_class("content-drawer")
        && widget.widget_name().starts_with("column-drawer-")) {
        add(&format!("canvas-panel.{}",panel.widget_name().trim_start_matches("column-drawer-")),"canvas-panel",Some(panel.clone()));
    }
    let mut dialog_measure = serde_json::Value::Null;
    let presented = w.window.visible_dialog().or_else(|| w.pages.visible()).or_else(|| if phone.is_none() { w.visible_dialog() } else { None });
    if let Some(dialog) = presented {
        let phone_page = phone.is_some() && w.pages.visible().as_ref() == Some(&dialog);
        let page = if phone_page { Some(crate::page_dialog::parent(&dialog)) }
            else { dialog.child().or_else(|| Some(dialog.clone().upcast())) };
        add("page","page",page.clone());
        if let Some(page) = page {
            let shell = |name: &str| widgets(&page).find(|widget| widget.is_mapped() && widget.widget_name() == name);
            let headers = widgets(&page).filter(|widget| widget.is_mapped() && widget.widget_name() == "dialog-header").collect::<Vec<_>>();
            if phone_page { assert!(headers.len() <= 1,"allocation export has duplicate mapped page headers"); }
            if phone_page || !headers.is_empty() { add("page.header","page-header",headers.first().cloned()); }
            let start = shell("dialog-start").or_else(|| headers.first().and_then(|header| widgets(header).filter(|widget| widget.is_mapped())
                .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                .find(|button| button.icon_name().as_deref() == Some("go-previous-symbolic")
                    || widgets(button.upcast_ref()).any(|widget| widget.downcast_ref::<gtk::Image>()
                        .is_some_and(|image| image.icon_name().as_deref() == Some("go-previous-symbolic"))))
                .map(|button| { button.set_widget_name("dialog-start"); button.upcast() })));
            let end = shell("dialog-end").or_else(|| crate::page_dialog::primary_button(&dialog)
                .filter(|button| button.is_mapped() && button.is_ancestor(&page)).map(|button| button.upcast()));
            if let Some(start) = start { add("page.start","page-action",Some(start)); }
            if let Some(end) = end.clone() { add("page.end","page-action",Some(end)); }
            let panes = shell("dialog-panes");
            if let Some(panes) = panes.clone() { add("page.panes","panes",Some(panes)); }
            let split = panes.as_ref().and_then(|panes| widgets(panes).find(|widget| widget.is::<adw::NavigationSplitView>()))
                .and_then(|widget| widget.downcast::<adw::NavigationSplitView>().ok());
            let sidebar = split.as_ref().and_then(|split| split.sidebar()).map(|page| page.upcast::<gtk::Widget>());
            let content = split.as_ref().and_then(|split| split.content()).map(|page| page.upcast::<gtk::Widget>());
            let first_group = descendants::<adw::PreferencesGroup>(&page).into_iter().find(|group| group.is_mapped());
            let width = |widget: &gtk::Widget| {
                let (minimum,natural,_,_) = widget.measure(gtk::Orientation::Horizontal,-1);
                serde_json::json!({"allocated":widget.width(),"minimum":minimum,"natural":natural})
            };
            let end_measure = end.as_ref().and_then(|widget| widget.downcast_ref::<gtk::Button>()).map(|button| {
                let label = widgets(button.upcast_ref()).filter_map(|widget| widget.downcast::<gtk::Label>().ok())
                    .find(|label| label.is_mapped());
                let padding = button.style_context().padding();
                let border = button.style_context().border();
                serde_json::json!({"button":width(button.upcast_ref()),"text":button.label().map(|text| text.to_string()),
                    "padding":[padding.left(),padding.top(),padding.right(),padding.bottom()],
                    "border":[border.left(),border.top(),border.right(),border.bottom()],
                    "label":label.map(|label| {
                        let font = label.pango_context().font_description();
                        serde_json::json!({"text":label.text().to_string(),"allocated":width(label.upcast_ref()),
                            "bounds":label.compute_bounds(button).map(|bounds| [bounds.x(),bounds.y(),bounds.width(),bounds.height()]),
                            "layout_pixels":label.layout().pixel_size(),"font":font.as_ref().map(ToString::to_string),
                            "weight":font.as_ref().map(|font| format!("{:?}",font.weight())),
                            "size":font.as_ref().map(|font| font.size() as f64 / gtk::pango::SCALE as f64),
                            "absolute":font.as_ref().map(|font| font.is_size_absolute())})
                    })})
            });
            dialog_measure = serde_json::json!({"page":width(&page),
                "panes":panes.as_ref().map(&width),
                "sidebar":sidebar.as_ref().map(&width),"content":content.as_ref().map(&width),
                "first_group":first_group.as_ref().map(|widget| width(widget.upcast_ref())),
                "end":end_measure,
                "collapsed":split.as_ref().map(|split| split.is_collapsed()),
                "shows_content":split.as_ref().map(|split| split.shows_content())});
            if dialog.widget_name() == "tool-picker" {
                let search = widgets(&page).find(|widget| widget.is::<gtk::SearchEntry>() && widget.widget_name() == "tool-search");
                add("picker.search","search",search.clone());
                if let Some(catalog) = search.and_then(|search| search.parent()) {
                    let body = catalog.parent();
                    add("picker.catalog","catalog",Some(catalog.clone()));
                    add("picker.body","body",body.clone());
                    let choices = widgets(&catalog).find(|widget| widget.is::<gtk::ScrolledWindow>());
                    add("picker.choices","scroll",choices.clone());
                    if let Some(row) = choices.as_ref().and_then(|choices| widgets(choices).find(|widget| widget.is::<gtk::ListBox>())
                        .and_then(|list| list.downcast::<gtk::ListBox>().ok())
                        .and_then(|list| list.row_at_index(0))
                        .filter(|row| row.compute_bounds(choices).is_some_and(|bounds| bounds.intersection(
                            &gtk::graphene::Rect::new(0.,0.,choices.width() as f32,choices.height() as f32)).is_some()))) {
                        let row = row.upcast::<gtk::Widget>();
                        add("picker.row.0","row",Some(row.clone()));
                        let mut labels = widgets(&row).filter(|widget| widget.is::<gtk::Label>());
                        add("picker.row.0.title","label",labels.next());
                        add("picker.row.0.subtitle","label",labels.next());
                    }
                }
            }
        }
    }
    drop(add);
    let text_pt = std::env::var("LAYER_PHONE_TEXT_PT").ok().map(|value| value.parse::<f32>().unwrap())
        .unwrap_or(layer_ui::UI_TEXT_PT as f32);
    let font = w.window.pango_context().font_description().map(|font| font.to_string().as_str().to_owned());
    let report = serde_json::json!({"schema_version":1,"host":"gtk","state":state_name,"viewport":viewport,"dpr":dpr,
        "theme":format!("{:?}",state.theme).to_lowercase(),"text_scale":text_pt as f64/layer_ui::UI_TEXT_PT as f64,
        "preset":w.workspaces.view().id,"phone_active":phone.is_some(),
        "status":if phone.is_none() && dialog_measure.is_null() { "unavailable" } else if missing.is_empty() { "captured" } else { "missing" },
        "reason":if phone.is_none() && dialog_measure.is_null() { Some("phone layout inactive") } else { None },"elements":elements,"missing":missing,
        "source":{"capture":capture_name,"coordinate_space":"window-client-logical","scale_factor":w.window.scale_factor(),
            "font":font,"text_pt":text_pt,"base_text_pt":layer_ui::UI_TEXT_PT,"requested":phone,
            "document":document,"camera":camera,
            "swapped":state.phone.swapped,"hidden":state.phone.hidden,"widgets":widget_sources,
            "tiles":state.workspace.layout.phone,"dialog_measure":dialog_measure}});
    let path = std::path::PathBuf::from(directory).join(format!("{capture_name}.allocations.json"));
    let bytes = serde_json::to_vec_pretty(&report).unwrap();
    glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || std::fs::write(path,bytes))).unwrap().unwrap();
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
