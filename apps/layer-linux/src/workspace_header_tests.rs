//! Actual GTK widgets and Mutter-delivered input for the window-bar builder.
use super::*;

fn assert_shared_icons(widget: &gtk::Widget) {
    if let Some(image) = widget.downcast_ref::<gtk::Image>()
        && let Some(name) = crate::icons::name(image).filter(|name| name.starts_with("layer-"))
    {
        assert!(
            image.paintable().is_some_and(|p| p.is::<gtk::Svg>()),
            "{name} must use the shared SVG renderer, not theme-symbolic loading"
        );
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        assert_shared_icons(&widget);
    }
}

struct Driver {
    w: Rc<Workspace>,
    _app: NativeTestApp,
    dir: std::path::PathBuf,
    step: usize,
}
impl Driver {
    fn managed(name: &str) -> Self {
        assert!(std::env::var_os("CAPY_WORKSPACE_DIR").is_some());
        let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
        let app = native_test_app(name);
        let w = Workspace::new(&app);
        w.window.maximize();
        w.window.present();
        Self::wait_ready(&w);
        pump(500);
        std::fs::write(dir.join("ready"), "ready").unwrap();
        pump(500);
        Self {
            w,
            _app: app,
            dir,
            step: 0,
        }
    }
    fn wait_ready(w: &Workspace) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !w.workspaces.ready.get() || w.workspaces.busy.get() {
            assert!(Instant::now() < deadline, "workspace startup or switch");
            pump(20);
        }
    }
    fn header_tool(&self, control: ToolbarControl) -> String {
        let id = state(&self.w)
            .workspace
            .layout
            .header
            .entries()
            .find(|e| e.item == HeaderItem::Tool { control })
            .unwrap()
            .id;
        format!("header-item-{id}")
    }
    fn number(&mut self, root: &gtk::Widget, text: &str) {
        self.click(&find_css(root, "number-value").unwrap());
        let entry = find_css(root, "number-entry")
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        assert!(entry.is_mapped());
        self.perform(serde_json::json!([{ "key": 0xffe3, "down": true }, { "key": 0x61, "down": true }, { "key": 0x61, "down": false }, { "key": 0xffe3, "down": false }]));
        for c in text.chars() {
            self.key(c as u32);
        }
        self.key(0xff0d);
    }
    fn new(name: &str) -> Self {
        let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
        let app = native_test_app(name);
        let w = fixture_workspace(&app);
        w.window.maximize();
        w.window.present();
        pump(1800);
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: WorkspaceState {
                layout: WorkspacePreset::Painter.layout(Platform::Gtk),
                ..WorkspaceState::default()
            },
        });
        pump(500);
        std::fs::write(dir.join("ready"), "ready").unwrap();
        pump(500);
        Self {
            w,
            _app: app,
            dir,
            step: 0,
        }
    }
    fn perform(&mut self, events: serde_json::Value) {
        let file = self.dir.join(format!("step-{}.json", self.step));
        let temporary = file.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(temporary, file).unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while !self.dir.join(format!("done-{}", self.step)).exists() {
            assert!(Instant::now() < deadline, "input step {}", self.step);
            pump(5);
        }
        self.step += 1;
        pump(180);
    }
    fn point(&self, widget: &gtk::Widget) -> [f32; 2] {
        assert!(widget.is_mapped(), "{} is not mapped", widget.widget_name());
        let b = widget.compute_bounds(&self.w.window).unwrap();
        assert!(b.width() > 0. && b.height() > 0.);
        [b.x() + b.width() / 2., b.y() + b.height() / 2.]
    }
    fn named(&self, name: &str) -> gtk::Widget {
        find_named(self.w.window.upcast_ref(), name).unwrap_or_else(|| panic!("missing {name}"))
    }
    fn click(&mut self, widget: &gtk::Widget) {
        let p = self.point(widget);
        self.perform(serde_json::json!([{ "point": p }, { "down": true }, { "down": false }]));
    }
    fn click_name(&mut self, name: &str) {
        self.click(&self.named(name));
    }
    fn label(&self, text: &str) -> gtk::Widget {
        fn find(root: &gtk::Widget, text: &str) -> Option<gtk::Widget> {
            if root.is_mapped()
                && root
                    .downcast_ref::<gtk::Label>()
                    .is_some_and(|l| l.text() == text)
            {
                return Some(root.clone());
            }
            let mut child = root.first_child();
            while let Some(w) = child {
                child = w.next_sibling();
                if let Some(found) = find(&w, text) {
                    return Some(found);
                }
            }
            None
        }
        find(self.w.window.upcast_ref(), text).unwrap_or_else(|| panic!("no visible label {text}"))
    }
    fn click_label(&mut self, text: &str) {
        self.click(&self.label(text));
    }
    fn key(&mut self, key: u32) {
        self.perform(
            serde_json::json!([{ "key": key, "down": true }, { "key": key, "down": false }]),
        );
    }
    fn edit(&mut self) {
        self.perform(serde_json::json!([{ "key": 0xffe3, "down": true }, { "key": 0xffe1, "down": true }, { "key": 0x75, "down": true }, { "key": 0x75, "down": false }, { "key": 0xffe1, "down": false }, { "key": 0xffe3, "down": false }]));
        assert!(state(&self.w).customization.header_editing);
    }
    fn finish(self) {
        std::fs::write(self.dir.join("finished"), "done").unwrap();
        self.w.window.close();
        pump(200);
    }
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_managed_input --native-storage"]
fn native_header_managed_input() {
    let mut d = Driver::managed("art.capycanvas.HeaderStorage");
    d.click_name("workspace-switch-painter");
    Driver::wait_ready(&d.w);
    pump(400);
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter());
    let switcher =
        d.w.workspaces
            .switcher
            .compute_bounds(&d.w.surface)
            .unwrap();
    assert!(
        (switcher.x() + switcher.width() / 2. - d.w.surface.width() as f32 / 2.).abs() < 1.,
        "visible workspace choices, not just their container, must be centered"
    );
    d.edit();
    d.named("header-size")
        .downcast::<gtk::DropDown>()
        .unwrap()
        .set_selected(2);
    pump(120);
    let capy = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .find(|e| e.item == HeaderItem::Capy)
        .unwrap()
        .id;
    d.named(&format!("header-item-{capy}")).grab_focus();
    d.key(0xffff);
    assert!(state(&d.w).workspace.layout.header.entry(capy).is_err());
    assert!(
        gtk::prelude::GtkWindowExt::focus(&d.w.window)
            .is_some_and(|f| f.widget_name().starts_with("header-item-")),
        "keyboard position survives removal"
    );
    d.click_name("header-options");
    d.click_label("Show Zoom and Rotation");
    d.click_name("header-zone-2");
    d.click_name("header-add-tools");
    d.named("tool-search")
        .downcast::<gtk::SearchEntry>()
        .unwrap()
        .set_text("Brush opacity");
    pump(300);
    d.click_name(&format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Opacity).unwrap()
    ));
    d.click_name("confirm-tools");
    assert!(state(&d.w).customization.header_editing);
    d.click_name("header-edit-done");
    let saved = durable_layout(&state(&d.w).workspace.layout);
    for name in ["illustrator", "painter"] {
        d.click_name(&format!("workspace-switch-{name}"));
        Driver::wait_ready(&d.w);
        pump(400);
    }
    assert_eq!(durable_layout(&state(&d.w).workspace.layout), saved);
    d.edit();
    d.named("header-size")
        .downcast::<gtk::DropDown>()
        .unwrap()
        .set_selected(0);
    pump(150);
    assert_ne!(state(&d.w).workspace.layout.header, saved.header);
    // Closing without Done must persist the committed workspace, not the preview.
    d.w.window.close();
    let deadline = Instant::now() + Duration::from_secs(20);
    while d.w.window.is_visible() {
        assert!(Instant::now() < deadline);
        pump(20);
    }
    d.w = Workspace::new(&d._app);
    d.w.window.maximize();
    d.w.window.present();
    Driver::wait_ready(&d.w);
    pump(500);
    assert_eq!(
        d.w.workspaces
            .manager
            .as_ref()
            .unwrap()
            .active_name()
            .as_deref(),
        Some("Painter")
    );
    assert_eq!(
        durable_layout(&state(&d.w).workspace.layout),
        saved,
        "restart preserves header and canvas info"
    );
    d.click_name(&d.header_tool(ToolbarControl::Color));
    assert!(state(&d.w).customization.drawer.is_some());
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::CloseExpanded,
    });
    pump(100);
    crate::capture(&d.w, d.dir.join("managed-painter.png").to_str().unwrap());
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_default_workspace_recovery_input --native-storage"]
fn native_default_workspace_recovery_input() {
    let mut d = Driver::managed("art.capycanvas.DefaultWorkspaceRecovery");
    let database = std::path::PathBuf::from(std::env::var_os("CAPY_WORKSPACE_DIR").unwrap())
        .join("workspaces.sqlite3");
    let sql = |query: &str| {
        let output = std::process::Command::new("sqlite3")
            .arg(&database)
            .arg(query)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    d.w.dispatch(UiAction::SetBrushSize { value: 77. });
    pump(200);
    let illustrator =
        d.w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .capture_workspace()
            .unwrap()
            .working;
    // The failing workspace is inactive, so its saved bytes cannot be autosaved
    // over by the live editor. "wheel" is not a supported ColorShape variant.
    sql(
        "UPDATE items SET working=json_set(working,'$.colors.shape','wheel') WHERE id='builtin:workspace:painter'",
    );
    d.click_name("workspace-switch-painter");
    Driver::wait_ready(&d.w);
    pump(400);
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter());
    assert!(d.w.workspaces.manager.as_ref().unwrap().error().is_none());
    assert_eq!(
        sql(
            "SELECT json_extract(working,'$.colors.shape') FROM items WHERE id='builtin:workspace:painter'"
        ),
        "circle"
    );
    d.click_name(&d.header_tool(ToolbarControl::Color));
    assert!(state(&d.w).customization.drawer.is_some());
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::CloseExpanded,
    });
    d.w.window.close();
    let deadline = Instant::now() + Duration::from_secs(20);
    while d.w.window.is_visible() {
        assert!(Instant::now() < deadline, "workspace close");
        pump(20);
    }
    // Startup repairs only the resumed Painter, not every included workspace.
    sql(
        "UPDATE items SET working=json_set(working,'$.colors.shape','wheel') WHERE id IN ('builtin:workspace:painter','builtin:workspace:photographer')",
    );
    d.w = Workspace::new(&d._app);
    d.w.window.maximize();
    d.w.window.present();
    Driver::wait_ready(&d.w);
    pump(500);
    assert_eq!(
        d.w.workspaces
            .manager
            .as_ref()
            .unwrap()
            .active_name()
            .as_deref(),
        Some("Painter")
    );
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter());
    assert!(d.w.area.is_mapped());
    assert_eq!(
        sql(
            "SELECT json_extract(working,'$.colors.shape') FROM items WHERE id='builtin:workspace:photographer'"
        ),
        "wheel"
    );
    d.w.workspaces
        .ui
        .show(&d.w, layer_workspace::ManagerPage::Workspaces);
    pump(500);
    d.click_name("workspace-row-builtin:workspace:photographer");
    pump(300);
    assert!(d.named("workspace-manager-apply").is_sensitive());
    assert_eq!(
        durable_layout(&state(&d.w).workspace.layout),
        WorkspacePreset::Photographer.layout(Platform::Gtk)
    );
    assert_eq!(
        d.w.workspaces
            .manager
            .as_ref()
            .unwrap()
            .active_name()
            .as_deref(),
        Some("Painter")
    );
    assert_eq!(
        sql("SELECT owner IS NULL FROM items WHERE id='builtin:workspace:photographer'"),
        "1"
    );
    let cancel = find_button(d.w.workspaces.ui.dialog.upcast_ref(), "Cancel").unwrap();
    d.click(cancel.upcast_ref());
    pump(250);
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter());
    for name in ["illustrator", "photographer", "painter"] {
        d.click_name(&format!("workspace-switch-{name}"));
        Driver::wait_ready(&d.w);
        pump(300);
        if name == "illustrator" {
            assert_eq!(
                d.w.gpu
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .session
                    .capture_workspace()
                    .unwrap()
                    .working,
                illustrator
            );
        }
    }
    assert_eq!(
        sql("SELECT count(*) FROM items WHERE json_extract(working,'$.colors.shape')='wheel'"),
        "0"
    );
    assert!(d.w.workspaces.manager.as_ref().unwrap().error().is_none());
    d.click_name(&d.header_tool(ToolbarControl::Color));
    assert!(state(&d.w).customization.drawer.is_some());
    crate::capture(&d.w, d.dir.join("recovered-painter.png").to_str().unwrap());
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_catalog_preview_input"]
fn native_header_catalog_preview_input() {
    let mut d = Driver::new("art.capycanvas.HeaderCatalog");
    let original = state(&d.w).workspace.layout.header;
    let capture =
        d.w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .capture_workspace()
            .unwrap();
    for (touch, grip, held) in [
        (false, false, false),
        (false, false, true),
        (false, true, false),
        (true, false, false),
        (true, false, true),
        (true, true, false),
    ] {
        d.edit();
        let name = if grip {
            "header-add-grip-clock"
        } else {
            "header-add-clock"
        };
        let source = d.point(&d.named(name));
        let destination = [d.w.surface.width() as f32 / 2., 30.];
        let mut events = if touch {
            vec![serde_json::json!({"touch":"down","point":source})]
        } else {
            vec![
                serde_json::json!({"point":source}),
                serde_json::json!({"down":true}),
            ]
        };
        if held {
            events.extend((0..12).map(|_| serde_json::json!({})));
        }
        events.extend(if touch {
            vec![
                serde_json::json!({"touch":"move","point":[source[0],source[1]-20.]}),
                serde_json::json!({"touch":"move","point":destination}),
                serde_json::json!({"touch":"up"}),
            ]
        } else {
            vec![
                serde_json::json!({"point":[source[0],source[1]-20.]}),
                serde_json::json!({"point":destination}),
                serde_json::json!({"down":false}),
            ]
        });
        d.perform(serde_json::Value::Array(events));
        let layout = state(&d.w).workspace.layout.header;
        assert_eq!(
            layout.entries().count(),
            original.entries().count() + usize::from(grip || held),
            "touch={touch} grip={grip} held={held}"
        );
        if grip || held {
            let entry = layout
                .entries()
                .find(|e| e.item == HeaderItem::Clock)
                .unwrap();
            assert_eq!(layout.location(entry.id).unwrap().0, HeaderZone::Center);
        }
        assert!(!d.w.dragging.get() && d.w.workspace_drag.borrow().is_none());
        assert_eq!(
            d.w.gpu
                .borrow_mut()
                .as_mut()
                .unwrap()
                .session
                .capture_workspace()
                .unwrap()
                .history,
            capture.history,
            "preview must not be autosaved"
        );
        d.click_name("header-edit-cancel");
        assert_eq!(state(&d.w).workspace.layout.header, original);
    }
    d.edit();
    // Cancel outside, Escape, blur and replacement without ever creating a tile.
    for cancellation in 0..4 {
        let source = d.point(&d.named("header-add-grip-clock"));
        let destination = [d.w.surface.width() as f32 / 2., 30.];
        d.perform(serde_json::json!([{"point":source},{"down":true},{"point":destination}]));
        assert!(d.w.dragging.get());
        match cancellation {
            0 => d.perform(serde_json::json!([{"point":[900.,700.]}])),
            1 => d.key(0xff1b),
            2 => {
                d.w.interact(UiInput::Blur);
            }
            _ => {
                d.w.dispatch(
                    HeaderAction::SetSize {
                        size: HeaderSize::Large,
                    }
                    .action(),
                );
                d.w.dispatch(
                    HeaderAction::SetSize {
                        size: original.size,
                    }
                    .action(),
                );
                pump(250);
            }
        }
        d.perform(serde_json::json!([{"down":false}]));
        assert!(!d.w.dragging.get() && d.w.workspace_drag.borrow().is_none());
        assert_eq!(state(&d.w).workspace.layout.header, original);
        assert!(state(&d.w).customization.header_editing);
    }
    // The ordinary tool picker adds into this preview, not directly to storage.
    d.click_name("header-zone-2");
    d.click_name("header-add-tools");
    d.named("tool-search")
        .downcast::<gtk::SearchEntry>()
        .unwrap()
        .set_text("Brush opacity");
    pump(250);
    d.click_name(&format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Opacity).unwrap()
    ));
    d.click_name("confirm-tools");
    assert!(state(&d.w).customization.header_editing);
    let preview = state(&d.w).workspace.layout.header;
    d.click_name("header-edit-done");
    assert_eq!(state(&d.w).workspace.layout.header, preview);
    let saved =
        d.w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .capture_workspace()
            .unwrap();
    assert_eq!(saved.history.generation, capture.history.generation + 1);
    d.edit();
    d.named("header-size")
        .downcast::<gtk::DropDown>()
        .unwrap()
        .set_selected(2);
    pump(120);
    d.key(0xff1b);
    assert!(!state(&d.w).customization.header_editing);
    assert_eq!(
        state(&d.w).workspace.layout.header,
        preview,
        "Escape cancels uncommitted edits"
    );
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_picker_journey"]
fn native_header_picker_journey() {
    let mut d = Driver::new("art.capycanvas.HeaderPickerJourney");
    let original = state(&d.w).workspace.layout.header;
    let opacity = format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Opacity).unwrap()
    );
    let color = format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Color).unwrap()
    );
    let search = |d: &Driver, query| {
        d.named("tool-search")
            .downcast::<gtk::SearchEntry>()
            .unwrap()
            .set_text(query);
        pump(300);
    };
    for touch in [false, true] {
        d.edit();
        let before = original.zones[2][0].id;
        let item = d.named(&format!("header-item-{before}"));
        let b = item.compute_bounds(&d.w.window).unwrap();
        let point = [b.x() + 25., b.y() + b.height() / 2.]; // Body, before its midpoint; not its grip.
        d.perform(if touch {
            serde_json::json!([{"touch":"down","point":point},{"touch":"up"}])
        } else {
            serde_json::json!([{"point":point},{"down":true},{"down":false}])
        });
        assert!(item.has_css_class("editing-selection"));
        assert_eq!(
            d.named("header-insertion-label")
                .downcast::<gtk::Label>()
                .unwrap()
                .text(),
            "Add to Right · before Brush"
        );
        d.click_name("header-add-tools");
        assert_shared_icons(d.w.window.visible_dialog().unwrap().upcast_ref());
        assert!(d.w.window.visible_dialog().is_some());
        assert!(!d.named("toolbar-name").is_mapped());
        assert!(!d.named("confirm-tools").is_sensitive());
        search(&d, "Brush opacity");
        d.click_name(&opacity);
        search(&d, "Color");
        d.click_name(&color);
        search(&d, "No tool matches this query");
        assert!(d.label("No matching tools").is_mapped());
        assert_eq!(
            state(&d.w)
                .customization
                .picker
                .as_ref()
                .unwrap()
                .selected
                .len(),
            2
        );
        assert!(
            d.named("confirm-tools").is_sensitive(),
            "Filtering retains selected tools"
        );
        crate::capture(
            &d.w,
            d.dir
                .join(format!("picker-empty-{touch}.png"))
                .to_str()
                .unwrap(),
        );
        if touch {
            d.click_name("cancel-tools");
        } else {
            d.key(0xff1b);
        }
        assert!(
            state(&d.w).customization.header_editing,
            "Cancel/Escape closes the picker, not its parent preview"
        );
        assert!(state(&d.w).customization.picker.is_none());
        assert_eq!(state(&d.w).workspace.layout.header, original);
        d.click_name("header-add-tools");
        assert_eq!(
            d.named("tool-search")
                .downcast::<gtk::SearchEntry>()
                .unwrap()
                .text(),
            ""
        );
        assert!(!d.named("confirm-tools").is_sensitive());
        search(&d, "Brush opacity");
        d.click_name(&opacity);
        search(&d, "Color");
        d.click_name(&color);
        crate::capture(
            &d.w,
            d.dir
                .join(format!("picker-selected-{touch}.png"))
                .to_str()
                .unwrap(),
        );
        d.click_name("confirm-tools");
        let added = state(&d.w).workspace.layout.header;
        assert_eq!(
            added.zones[2][0].item,
            HeaderItem::Tool {
                control: ToolbarControl::Opacity
            }
        );
        assert_eq!(
            added.zones[2][1].item,
            HeaderItem::Tool {
                control: ToolbarControl::Color
            }
        );
        assert_eq!(added.zones[2][2].id, before);
        assert!(state(&d.w).customization.header_editing);
        d.click_name("header-zone-1");
        d.click_name("header-add-clock");
        let clock = state(&d.w)
            .workspace
            .layout
            .header
            .entries()
            .find(|e| e.item == HeaderItem::Clock)
            .unwrap()
            .id;
        assert_eq!(
            state(&d.w)
                .workspace
                .layout
                .header
                .location(clock)
                .unwrap()
                .0,
            HeaderZone::Center
        );
        assert!(!d.named("header-add-clock").is_mapped());
        d.click_name(&format!("header-item-{clock}"));
        assert!(d.named("header-remove-item").is_sensitive());
        d.click_name("header-remove-item");
        assert!(d.named("header-add-clock").is_mapped());
        // Keyboard users choose an exact slot using a focused item and Enter.
        let target = d.named(&format!("header-item-{before}"));
        target.grab_focus();
        d.key(0xff0d);
        d.click_name("header-add-space");
        let current = state(&d.w).workspace.layout.header;
        let (_, index) = current.location(before).unwrap();
        assert_eq!(current.zones[2][index - 1].item, HeaderItem::Space);
        crate::capture(
            &d.w,
            d.dir
                .join(format!("editor-insertion-{touch}.png"))
                .to_str()
                .unwrap(),
        );
        d.click_name("header-edit-cancel");
        assert_eq!(state(&d.w).workspace.layout.header, original);
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_spacing_visual"]
fn native_header_spacing_visual() {
    let mut d = Driver::managed("art.capycanvas.HeaderSpacing");
    d.click_name("workspace-switch-painter");
    Driver::wait_ready(&d.w);
    pump(250);
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for size in HeaderSize::ALL {
            d.w.dispatch(HeaderAction::SetSize { size }.action());
            pump(250);
            assert_shared_icons(d.w.header.root.upcast_ref());
            let close = find_css(d.w.header.root.upcast_ref(), "close").unwrap();
            let b = close.compute_bounds(&d.w.surface).unwrap();
            assert!(
                (b.width() - b.height()).abs() < 1.,
                "{size:?}: close not square: {b:?}"
            );
            assert!(
                (b.y() - 6.).abs() < 1.
                    && (d.w.header.height() - b.y() - b.height() - 6.).abs() < 1.,
                "close vertical padding: {b:?}"
            );
            let image = close.first_child().unwrap().compute_bounds(&close).unwrap();
            assert!((image.x() - (close.width() as f32 - image.x() - image.width())).abs() < 1.);
            assert!((image.y() - (close.height() as f32 - image.y() - image.height())).abs() < 1.);
            let h = state(&d.w).workspace.layout.header;
            for pair in h.zones[2].windows(2) {
                let a = d
                    .named(&format!("header-item-{}", pair[0].id))
                    .compute_bounds(&d.w.surface)
                    .unwrap();
                let b = d
                    .named(&format!("header-item-{}", pair[1].id))
                    .compute_bounds(&d.w.surface)
                    .unwrap();
                assert!(
                    (b.x() - a.x() - a.width() - 6.).abs() < 1.,
                    "tile gap {a:?} {b:?}"
                );
            }
            let switcher =
                d.w.workspaces
                    .switcher
                    .compute_bounds(&d.w.surface)
                    .unwrap();
            assert!(
                (switcher.x() + switcher.width() / 2. - d.w.surface.width() as f32 / 2.).abs() < 1.
            );
            assert!(switcher.height() <= 36., "selector stretched vertically");
            crate::capture(
                &d.w,
                d.dir
                    .join(format!("bar-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            let tool = d.named(&d.header_tool(ToolbarControl::Color));
            d.click(&tool);
            let button = find_css(&tool, "header-tool").unwrap();
            assert!(button.has_css_class("drawer-origin-bottom"));
            assert!(button.has_css_class("drawer-open"));
            assert!(!button.has_css_class("selected-tool"));
            crate::capture(
                &d.w,
                d.dir
                    .join(format!("drawer-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            d.click(&tool);
            assert!(!button.has_css_class("drawer-origin-bottom"));
            d.edit();
            assert_shared_icons(d.w.header.root.upcast_ref());
            let capy = h.entries().find(|e| e.item == HeaderItem::Capy).unwrap().id;
            let item = d.named(&format!("header-item-{capy}"));
            let grip = d
                .named(&format!("header-grip-{capy}"))
                .compute_bounds(&item)
                .unwrap();
            assert!(
                (grip.y() + grip.height() / 2. - item.height() as f32 / 2.).abs() < 1.,
                "grip alignment"
            );
            crate::capture(
                &d.w,
                d.dir
                    .join(format!("editor-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            d.click_name("header-edit-cancel");
        }
    }
    d.click_name("workspace-switch-illustrator");
    Driver::wait_ready(&d.w);
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(200);
        let label = d.label("File");
        let mut button = label.clone();
        while !button.is::<gtk::Button>() {
            button = button.parent().unwrap();
        }
        let button_bounds = button.compute_bounds(&d.w.surface).unwrap();
        assert_eq!(
            button_bounds.height(),
            36.,
            "menu text buttons retain baseline height at {size:?}"
        );
        let label_bounds = label.compute_bounds(&d.w.surface).unwrap();
        assert!(
            label_bounds.x() - button_bounds.x() >= 6.
                && button_bounds.x() + button_bounds.width()
                    - label_bounds.x()
                    - label_bounds.width()
                    >= 6.
        );
        let p = d.point(&label);
        d.perform(serde_json::json!([{"point":p}]));
        crate::capture(
            &d.w,
            d.dir
                .join(format!("menu-hover-{size:?}.png"))
                .to_str()
                .unwrap(),
        );
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_drawer_dismissal_input"]
fn native_drawer_dismissal_input() {
    fn contact(d: &mut Driver, point: [f32; 2], touch: bool) {
        d.perform(if touch {
            serde_json::json!([{"touch":"down","point":point},{"touch":"up"}])
        } else {
            serde_json::json!([{"point":point},{"down":true},{"down":false}])
        });
    }
    fn tap(d: &mut Driver, widget: &gtk::Widget, touch: bool) {
        contact(d, d.point(widget), touch);
    }
    let mut d = Driver::new("art.capycanvas.DrawerDismissal");
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for touch in [false, true] {
            d.w.dispatch(UiAction::RestoreWorkspace {
                workspace: WorkspaceState {
                    layout: WorkspacePreset::Painter.layout(Platform::Gtk),
                    ..WorkspaceState::default()
                },
            });
            pump(250);
            let color = d.named(&d.header_tool(ToolbarControl::Color));
            let color_button = find_css(&color, "header-tool").unwrap();
            let brush = d.named(&d.header_tool(ToolbarControl::Command {
                command: CommandId::Brush,
            }));
            let menu = d.named("header-item-2");
            for outside in ["gap", "menu", "disabled", "canvas"] {
                tap(&mut d, &color, touch);
                assert!(state(&d.w).customization.drawer.is_some());
                assert!(!color_button.has_css_class("selected-tool"));
                assert!(color_button.has_css_class("drawer-open"));
                match outside {
                    "gap" => contact(&mut d, [420., 25.], touch),
                    "menu" => tap(&mut d, &menu, touch),
                    "disabled" => {
                        let transform = d.named(&d.header_tool(ToolbarControl::Command {
                            command: CommandId::ScaleRotate,
                        }));
                        tap(&mut d, &transform, touch);
                    }
                    _ => contact(&mut d, [800., 700.], touch),
                }
                assert!(
                    state(&d.w).customization.drawer.is_none(),
                    "{theme:?}/{touch}/{outside}"
                );
                assert!(!color_button.has_css_class("drawer-open"));
                if outside == "menu" {
                    assert!(
                        d.w.popovers
                            .borrow()
                            .iter()
                            .filter_map(|p| p.upgrade())
                            .any(|p| p.is_visible()),
                        "The same click must open the menu, not just dismiss the drawer"
                    );
                    d.key(0xff1b);
                }
                assert!(
                    !state(&d.w)
                        .commands
                        .iter()
                        .find(|c| c.id == CommandId::Undo)
                        .unwrap()
                        .enabled,
                    "Dismissing on the canvas must not leave a mark"
                );
            }
            tap(&mut d, &color, touch);
            tap(&mut d, &brush, touch);
            assert!(
                find_css(&brush, "header-tool")
                    .unwrap()
                    .has_css_class("selected-tool")
            );
            assert!(
                state(&d.w).customization.drawer.is_some(),
                "Switch drawer and select in one click"
            );
            tap(&mut d, &brush, touch);
            assert!(
                state(&d.w).customization.drawer.is_none(),
                "Current opener toggles closed"
            );
            // Docked and floating toolbar bodies also dismiss without consuming
            // their normal grip/title-bar click or a menu's activation.
            for floating in [false, true] {
                let mut layout = DockLayout::default();
                layout
                    .move_panel(
                        [1600., 1000.],
                        Panel::Toolbar,
                        if floating {
                            DockTarget::Float {
                                position: [420., 300.],
                            }
                        } else {
                            DockTarget::Edge {
                                edge: Edge::Top,
                                outer: true,
                            }
                        },
                    )
                    .unwrap();
                let color_id = layout
                    .panel(Panel::Toolbar)
                    .unwrap()
                    .tiles()
                    .iter()
                    .find(|t| t.control == ToolbarControl::Color)
                    .unwrap()
                    .id;
                d.w.dispatch(UiAction::RestoreWorkspace {
                    workspace: WorkspaceState {
                        layout,
                        ..WorkspaceState::default()
                    },
                });
                pump(250);
                let color =
                    d.w.customization
                        .drawer_button(TileAnchor {
                            panel: Panel::Toolbar,
                            tile: color_id,
                        })
                        .unwrap();
                tap(&mut d, color.upcast_ref(), touch);
                assert!(state(&d.w).customization.drawer.is_some());
                assert!(
                    !color.has_css_class("selected-tool") && color.has_css_class("drawer-open")
                );
                let group =
                    d.w.resolved()
                        .groups
                        .into_iter()
                        .find(|g| g.panels.contains(&Panel::Toolbar))
                        .unwrap();
                let handle = {
                    let groups = d.w.groups.borrow();
                    let view = groups.iter().find(|g| g.id == group.id).unwrap();
                    find_css(view.root.upcast_ref(), "panel-grip").unwrap()
                };
                tap(&mut d, &handle, touch);
                assert!(
                    state(&d.w).customization.drawer.is_none(),
                    "toolbar handle {floating}/{touch}"
                );
                tap(&mut d, color.upcast_ref(), touch);
                contact(&mut d, [500., 18.], touch);
                assert!(
                    state(&d.w).customization.drawer.is_none(),
                    "toolbar to window bar {floating}/{touch}"
                );
            }
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_cancel_caption_input"]
fn native_header_cancel_caption_input() {
    let mut d = Driver::new("art.capycanvas.HeaderCancellation");
    let original = state(&d.w).workspace.layout.header;
    // Native caption double-click remains owned by the window manager.
    d.perform(serde_json::json!([{ "point": [400., 20.] }, { "down": true }, { "down": false }, { "down": true }, { "down": false }]));
    assert!(!d.w.window.is_maximized());
    d.w.window.maximize();
    pump(400);
    d.edit();
    let id = original.zones[0][0].id;
    d.key(0xff09);
    assert!(
        state(&d.w).customization.header_editing && !state(&d.w).workspace.zen_mode,
        "Tab navigates the editor instead of hiding it"
    );
    for blur in [false, true] {
        let start = d.point(&d.named(&format!("header-grip-{id}")));
        d.perform(
            serde_json::json!([{ "point": start }, { "down": true }, { "point": [800.,start[1]] }]),
        );
        assert!(d.w.dragging.get());
        if blur {
            d.w.interact(UiInput::Blur);
        } else {
            d.key(0xff1b);
        }
        d.perform(serde_json::json!([{ "down": false }]));
        assert!(!d.w.dragging.get() && d.w.workspace_drag.borrow().is_none());
        assert_eq!(state(&d.w).workspace.layout.header, original);
        assert!(state(&d.w).customization.header_editing);
    }
    let start = d.point(&d.named(&format!("header-grip-{id}")));
    d.perform(
        serde_json::json!([{ "point": start }, { "down": true }, { "point": [800.,start[1]] }]),
    );
    d.w.dispatch(HeaderAction::Remove { id }.action());
    pump(120);
    assert!(
        !d.w.dragging.get() && d.w.workspace_drag.borrow().is_none(),
        "source replacement retires the contact"
    );
    d.perform(serde_json::json!([{ "down": false }]));
    d.w.dispatch(HeaderAction::Cancel.action());
    d.w.dispatch(HeaderAction::Edit { editing: true }.action());
    pump(120);
    assert_eq!(state(&d.w).workspace.layout.header, original);
    d.click_name("header-edit-done");
    let menu = original
        .entries()
        .find(|e| e.item == HeaderItem::Menu)
        .unwrap()
        .id;
    d.click_name(&format!("header-item-{menu}"));
    d.click_label("Window");
    d.click_label("Show Menu Bar");
    assert!(state(&d.w).workspace.layout.header.show_menu_labels);
    let labels = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .find(|e| e.item == HeaderItem::MenuLabels)
        .unwrap()
        .id;
    assert!(
        d.named(&format!("header-item-{labels}")).is_mapped(),
        "Show Menu Bar exposes menu names at desktop width"
    );
    // Structural publication must not be dropped when settings change in the
    // same frame (native refreshes used to overwrite a queued layout update).
    d.w.dispatch(
        HeaderAction::SetSize {
            size: HeaderSize::Large,
        }
        .action(),
    );
    d.w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Light),
    });
    pump(250);
    assert_eq!(d.w.header.root.height(), HeaderSize::Large.height() as i32);
    assert_eq!(
        state(&d.w).workspace.layout.header_presentation.height,
        HeaderSize::Large.height()
    );
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: WorkspaceState {
            layout: WorkspacePreset::Painter.layout(Platform::Gtk),
            ..WorkspaceState::default()
        },
    });
    pump(250);
    let capy = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .find(|e| e.item == HeaderItem::Capy)
        .unwrap()
        .id;
    d.click_name(&format!("header-item-{capy}"));
    d.perform(serde_json::json!([{ "point": [600.,400.] }]));
    assert!(state(&d.w).workspace.zen_mode && !d.w.header.root.can_target());
    d.perform(serde_json::json!([{ "point": [6.,6.] }]));
    assert!(d.w.header.root.can_target());
    d.click_name(&format!("header-item-{capy}"));
    assert!(!state(&d.w).workspace.zen_mode);
    // Close is native and cannot be removed by customization.
    let close = find_css(d.w.header.root.upcast_ref(), "close").unwrap();
    d.click(&close);
    assert!(!d.w.window.is_visible());
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_drawer_controls_input"]
fn native_header_drawer_controls_input() {
    let mut d = Driver::new("art.capycanvas.HeaderDrawers");
    let brush = ToolbarControl::Command {
        command: CommandId::Brush,
    };
    d.click_name(&d.header_tool(brush));
    if state(&d.w).customization.drawer.is_none() {
        d.click_name(&d.header_tool(brush));
    }
    let drawer = d.named("tool-drawer");
    d.number(&find_named(&drawer, "tool-setting-size").unwrap(), "37");
    assert_eq!(state(&d.w).brush.diameter, 37.);
    // The first click on another header tool switches the existing drawer.
    d.click_name(&d.header_tool(ToolbarControl::Command {
        command: CommandId::Blend,
    }));
    assert!(state(&d.w).customization.drawer.is_some());
    assert!(
        tool_state(
            &state(&d.w),
            ToolbarControl::Command {
                command: CommandId::Blend
            }
        )
        .1
    );
    d.click_name(&d.header_tool(ToolbarControl::Color));
    let before = state(&d.w).colors;
    let drawer = d.named("tool-drawer");
    d.click(&find_named(&drawer, "color-swap").unwrap());
    assert_eq!(state(&d.w).colors.foreground, before.background);
    d.click_name(&d.header_tool(ToolbarControl::Panel {
        panel: Panel::Layers,
    }));
    let layers = d.w.drawer.layers().unwrap();
    d.click(&layers.footer.last_child().unwrap());
    assert!(
        d.w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .any(|p| p.is_mapped() && p.parent().as_ref() == Some(layers.root.upcast_ref())),
        "Layers context menu uses its header drawer projection"
    );
    d.key(0xff1b);
    // Every content-panel family also works from an individual header item.
    let controls = Panel::ALL
        .into_iter()
        .filter(|p| p.kind() == PanelKind::Content && p.available_on(Platform::Gtk))
        .map(|panel| ToolbarControl::Panel { panel })
        .chain([
            ToolbarControl::Opacity,
            ToolbarControl::Brush {
                id: layer_core::DefaultBrushPreset::GPen as u32,
            },
            ToolbarControl::Size { pixels: 32 },
        ]);
    for control in controls {
        let mut workspace = WorkspaceState {
            layout: WorkspacePreset::Painter.layout(Platform::Gtk),
            ..WorkspaceState::default()
        };
        workspace.layout.header.zones = Default::default();
        workspace
            .layout
            .header
            .add(
                HeaderZone::Left,
                None,
                &[HeaderItem::Menu, HeaderItem::Tool { control }],
            )
            .unwrap();
        d.w.dispatch(UiAction::RestoreWorkspace { workspace });
        pump(160);
        let name = d.header_tool(control);
        d.click_name(&name);
        if !matches!(control, ToolbarControl::Size { .. }) {
            if state(&d.w).customization.drawer.is_none() {
                d.click_name(&name);
            }
            assert!(d.named("tool-drawer").is_mapped(), "{control:?}");
            let bounds = d.named("tool-drawer").compute_bounds(&d.w.surface).unwrap();
            assert!(bounds.y() >= d.w.header.height() && bounds.x() >= 0.);
            d.click_name(&name);
            assert!(
                state(&d.w).customization.drawer.is_none(),
                "repeat click closes {control:?}"
            );
        } else if let ToolbarControl::Size { pixels } = control {
            assert_eq!(state(&d.w).brush.diameter, pixels as f32);
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_hold_context_input"]
fn native_header_hold_context_input() {
    let mut d = Driver::new("art.capycanvas.HeaderHolds");
    let original = state(&d.w).workspace;
    let id = original.layout.header.zones[0][1].id;
    d.edit();
    for touch in [false, true] {
        let point = d.point(&d.named(&format!("header-item-{id}")));
        let mut events = if touch {
            vec![serde_json::json!({"touch":"down", "point":point})]
        } else {
            vec![
                serde_json::json!({"point":point}),
                serde_json::json!({"down":true}),
            ]
        };
        events.extend((0..12).map(|_| serde_json::json!({})));
        events.push(if touch {
            serde_json::json!({"touch":"up"})
        } else {
            serde_json::json!({"down":false})
        });
        d.perform(serde_json::Value::Array(events));
        assert_eq!(
            d.w.popovers
                .borrow()
                .iter()
                .filter_map(|p| p.upgrade())
                .any(|p| p.is_visible()),
            touch,
            "only a touch/pen hold opens a menu; mouse holds only arm pickup"
        );
        assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
        if touch {
            d.key(0xff1b);
        }
    }
    for touch in [false, true] {
        for grip in [false, true] {
            for held in [false, true] {
                if grip && held {
                    continue;
                }
                d.w.dispatch(UiAction::RestoreWorkspace {
                    workspace: original.clone(),
                });
                pump(160);
                d.w.dispatch(HeaderAction::Edit { editing: true }.action());
                pump(160);
                let from = d.point(&d.named(&format!(
                    "header-{}-{id}",
                    if grip { "grip" } else { "item" }
                )));
                let to = [d.w.surface.width() as f32 / 2., from[1]];
                let mut events = if touch {
                    vec![serde_json::json!({"touch":"down", "point":from})]
                } else {
                    vec![
                        serde_json::json!({"point":from}),
                        serde_json::json!({"down":true}),
                    ]
                };
                if held {
                    events.extend((0..12).map(|_| serde_json::json!({})));
                }
                if touch {
                    events.extend([
                        serde_json::json!({"touch":"move", "point":[from[0]+20.,from[1]]}),
                        serde_json::json!({"touch":"move", "point":to}),
                        serde_json::json!({"touch":"up"}),
                    ]);
                } else {
                    events.extend([
                        serde_json::json!({"point":[from[0]+20.,from[1]]}),
                        serde_json::json!({"point":to}),
                        serde_json::json!({"down":false}),
                    ]);
                }
                d.perform(serde_json::Value::Array(events));
                assert_eq!(
                    state(&d.w).workspace.layout.header.location(id).unwrap().0,
                    if held || grip {
                        HeaderZone::Center
                    } else {
                        HeaderZone::Left
                    },
                    "touch={touch} grip={grip} held={held}"
                );
                if held || grip {
                    d.w.dispatch(HeaderAction::Cancel.action());
                    pump(160);
                    assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
                }
                assert!(
                    !d.w.popovers
                        .borrow()
                        .iter()
                        .filter_map(|p| p.upgrade())
                        .any(|p| p.is_visible()),
                    "drag must dismiss the held menu"
                );
            }
        }
    }
    d.w.dispatch(HeaderAction::Edit { editing: false }.action());
    pump(160);
    let p = d.point(&d.named(&format!("header-item-{id}")));
    d.perform(serde_json::json!([{ "point": p }, { "button": 273, "down": true }, { "button": 273, "down": false }]));
    d.click_label("Move to Center");
    assert_eq!(
        state(&d.w).workspace.layout.header.location(id).unwrap().0,
        HeaderZone::Center
    );
    d.edit();
    let item = d.named(&format!("header-item-{id}"));
    item.grab_focus();
    d.perform(serde_json::json!([{ "key": 0xffe1, "down": true }, { "key": 0xffc7, "down": true }, { "key": 0xffc7, "down": false }, { "key": 0xffe1, "down": false }]));
    d.click_label("Remove from Window Bar");
    assert!(state(&d.w).workspace.layout.header.entry(id).is_err());
    d.w.dispatch(HeaderAction::Cancel.action());
    pump(160);
    assert!(!state(&d.w).customization.header_editing);
    assert!(state(&d.w).workspace.layout.header.entry(id).is_ok());
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_editor_controls_input"]
fn native_header_editor_controls_input() {
    let mut d = Driver::new("art.capycanvas.HeaderControls");
    d.edit();
    for size in HeaderSize::ALL {
        let dropdown = d.named("header-size").downcast::<gtk::DropDown>().unwrap();
        dropdown.set_selected(size as u32);
        pump(220);
        assert_eq!(state(&d.w).workspace.layout.header.size, size);
        let editor = d.named("header-editor");
        assert!(
            editor.height() < 240,
            "The inline editor is compact at {size:?}"
        );
        assert!(d.w.header.root.height() < size.height() as i32 + 300);
        assert_eq!(d.w.area.height(), d.w.surface.height());
    }
    for name in ["clock", "menu-labels"] {
        d.click_name(&format!("header-add-{name}"));
    }
    d.click_name("header-add-tools");
    d.named("tool-search")
        .downcast::<gtk::SearchEntry>()
        .unwrap()
        .set_text("Brush opacity");
    pump(250);
    d.click_name(&format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Opacity).unwrap()
    ));
    d.click_name("confirm-tools");
    for label in ["Brush opacity", "Clock", "Menu Labels"] {
        assert!(
            state(&d.w)
                .workspace
                .layout
                .header
                .entries()
                .any(|e| e.item.label() == label)
        );
    }
    assert!(
        !d.named("header-add-clock").is_mapped(),
        "Existing singleton components do not clutter the palette"
    );
    d.click_name("header-options");
    d.click_label("Show Zoom and Rotation");
    assert!(state(&d.w).workspace.layout.canvas_info.visible && d.w.view_info.is_visible());
    assert!(d.w.resolved().status.y > d.w.surface.height() as f32 / 2.);
    assert_eq!(d.w.view_info.halign(), gtk::Align::End);
    d.click_name("header-options");
    d.click_label("Restore Window Bar Defaults");
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::default());
    // Removing every editable navigation item cannot strand touch users.
    let ids = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .map(|e| e.id)
        .collect::<Vec<_>>();
    for id in ids {
        d.w.dispatch(HeaderAction::Remove { id }.action());
    }
    pump(250);
    d.click_name("header-edit-done");
    assert!(d.named("header-recovery").is_mapped());
    d.click_name("header-recovery");
    d.click_label("Window");
    d.click_label("Customize Workspace UI…");
    assert!(state(&d.w).customization.header_editing);
    crate::capture(&d.w, d.dir.join("empty-bar-palette.png").to_str().unwrap());
    for (i, item) in HeaderItem::COMPONENTS.into_iter().enumerate() {
        let zone = HeaderZone::ALL[i % 3];
        d.click_name(&format!("header-zone-{}", zone.index()));
        let name = format!(
            "header-add-{}",
            item.label().to_lowercase().replace(' ', "-")
        );
        d.click_name(&name);
        let model = state(&d.w).workspace.layout.header;
        let id = model.entries().find(|e| e.item == item).unwrap().id;
        assert_eq!(model.location(id).unwrap().0, zone);
        assert_eq!(d.named(&name).is_mapped(), !item.singleton());
    }
    crate::capture(
        &d.w,
        d.dir.join("all-components-added.png").to_str().unwrap(),
    );
    d.click_name("header-edit-done");
    d.finish();
}

#[test]
#[ignore = "640px isolated compositor, --native-test=native_header_overflow_input"]
fn native_header_overflow_input() {
    let mut d = Driver::new("art.capycanvas.HeaderOverflow");
    assert!(
        d.w.surface.width() <= 800,
        "run with LAYER_MOTION_VIEWPORT=640x600"
    );
    let baseline = state(&d.w).workspace.layout.header;
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(240);
        let model = state(&d.w).workspace.layout.header;
        for zone in HeaderZone::ALL {
            for entry in &model.zones[zone.index()] {
                if !d.named(&format!("header-item-{}", entry.id)).is_mapped() {
                    let overflow = d.named(&format!("header-overflow-{}", zone.index()));
                    d.click(&overflow);
                    let row = d.named(&format!("header-overflow-item-{}", entry.id));
                    if !row.is_mapped() {
                        crate::capture(&d.w, d.dir.join("overflow-failure.png").to_str().unwrap());
                    }
                    assert!(
                        row.is_mapped(),
                        "hidden item {} must be reachable at {size:?}",
                        entry.item.label()
                    );
                    if row.is_sensitive() {
                        d.click(&row);
                        if matches!(
                            entry.item,
                            HeaderItem::Tool {
                                control: ToolbarControl::Color | ToolbarControl::Panel { .. }
                            }
                        ) {
                            assert!(
                                state(&d.w).customization.drawer.is_some(),
                                "overflow drawer for {:?}",
                                entry.item
                            );
                            pump(400);
                            assert!(
                                state(&d.w).customization.drawer.is_some(),
                                "overflow origin must survive a new allocation"
                            );
                            assert!(
                                find_named(d.w.window.upcast_ref(), "tool-drawer")
                                    .unwrap()
                                    .is_mapped()
                            );
                        }
                    }
                    d.w.dispatch(UiAction::Customize {
                        action: CustomizationAction::CloseExpanded,
                    });
                    d.key(0xff1b);
                }
            }
        }
        d.w.dispatch(HeaderAction::Edit { editing: true }.action());
        pump(160);
        assert!(d.named("header-edit-done").is_mapped());
        crate::capture(
            &d.w,
            d.dir
                .join(format!("narrow-editor-{size:?}.png"))
                .to_str()
                .unwrap(),
        );
        if let Some(entry) = model
            .entries()
            .find(|e| !d.named(&format!("header-item-{}", e.id)).is_mapped())
        {
            let zone = model.location(entry.id).unwrap().0;
            d.click_name(&format!("header-overflow-{}", zone.index()));
            d.click_name(&format!("header-overflow-item-{}", entry.id));
            assert!(
                d.named("header-insertion-label")
                    .downcast::<gtk::Label>()
                    .unwrap()
                    .text()
                    .contains(&format!("before {}", entry.item.label()))
            );
            assert!(d.named("header-remove-item").is_sensitive());
            d.click_name("header-add-tools");
            assert!(d.w.window.visible_dialog().is_some());
            crate::capture(
                &d.w,
                d.dir
                    .join(format!("narrow-picker-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            d.key(0xff1b);
            assert!(state(&d.w).customization.header_editing);
            assert!(state(&d.w).customization.picker.is_none());
            assert!(d.w.window.visible_dialog().is_none());
        }
        d.click_name("header-edit-done");
        assert!(!state(&d.w).customization.header_editing);
    }
    assert_eq!(
        state(&d.w).workspace.layout.header.zones,
        baseline.zones,
        "overflow never changes the stored item arrangement"
    );
    crate::capture(&d.w, d.dir.join("narrow.png").to_str().unwrap());
    d.finish();
}

#[test]
#[ignore = "isolated compositor, --native-test=native_header_canvas_visual"]
fn native_header_canvas_visual() {
    let d = Driver::new("art.capycanvas.HeaderCanvas");
    for _ in 0..8 {
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::ZoomIn,
        });
    }
    pump(500);
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let _warm = crate::snapshot(&d.w);
        pump(120);
        let snapshot = crate::snapshot(&d.w);
        assert_eq!(snapshot.width(), d.w.surface.width());
        assert_eq!(snapshot.height(), d.w.surface.height());
        let mut bytes = vec![0u8; snapshot.width() as usize * snapshot.height() as usize * 4];
        snapshot.download(&mut bytes, snapshot.width() as usize * 4);
        let offset = (20 * snapshot.width() as usize + 400) * 4;
        assert!(
            bytes[offset..offset + 3].iter().all(|v| *v > 245),
            "white artwork must remain visible beneath the window bar"
        );
        snapshot
            .save_to_png(d.dir.join(format!("canvas-behind-{theme:?}.png")))
            .unwrap();
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_builder_input"]
fn native_header_builder_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.HeaderBuilder");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1800);
    w.dispatch(UiAction::RestoreWorkspace {
        workspace: WorkspaceState {
            layout: WorkspacePreset::Painter.layout(Platform::Gtk),
            ..WorkspaceState::default()
        },
    });
    pump(600);
    let original = state(&w).workspace.layout.header;
    assert_eq!(w.area.height(), w.surface.height());
    assert!(!w.view_info.is_visible());
    assert_eq!(w.header.root.height(), 60);
    let point = |name: &str| {
        let widget =
            find_named(w.window.upcast_ref(), name).unwrap_or_else(|| panic!("missing {name}"));
        assert!(widget.is_mapped(), "{name} must be mapped");
        let b = widget.compute_bounds(&w.surface).unwrap();
        let point = [b.x() + b.width() / 2., b.y() + b.height() / 2.];
        let picked = w
            .surface
            .pick(point[0].into(), point[1].into(), gtk::PickFlags::DEFAULT)
            .expect("hit target");
        assert!(
            picked == widget || picked.is_ancestor(&widget),
            "{name} is obscured by {}",
            picked.widget_name()
        );
        point
    };
    for entry in original.entries() {
        let _ = point(&format!("header-item-{}", entry.id));
    }
    crate::capture(&w, dir.join("painter.png").to_str().unwrap());
    let mut step = 0;
    let mut perform = |events: serde_json::Value| {
        let file = dir.join(format!("step-{step}.json"));
        let temporary = file.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(temporary, file).unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while !dir.join(format!("done-{step}")).exists() {
            assert!(Instant::now() < deadline, "native input step {step}");
            pump(5);
        }
        step += 1;
        pump(120);
    };
    std::fs::write(dir.join("ready"), "ready").unwrap();
    pump(500);
    let click = |p: [f32; 2]| serde_json::json!([{ "point": p }, { "button": 272, "down": true }, { "button": 272, "down": false }]);
    for control in [
        ToolbarControl::Command {
            command: CommandId::Eraser,
        },
        ToolbarControl::Command {
            command: CommandId::Blend,
        },
        ToolbarControl::Command {
            command: CommandId::Brush,
        },
    ] {
        let id = original
            .entries()
            .find(|e| e.item == HeaderItem::Tool { control })
            .unwrap()
            .id;
        perform(click(point(&format!("header-item-{id}"))));
        assert!(
            tool_state(&state(&w), control).1,
            "selected header tool {control:?}"
        );
        perform(click(point(&format!("header-item-{id}"))));
        assert!(
            state(&w).customization.drawer.is_some(),
            "header tool drawer {control:?}"
        );
        assert!(find_named(w.window.upcast_ref(), "tool-drawer").is_some_and(|d| d.is_mapped()));
        perform(click(point(&format!("header-item-{id}"))));
        assert!(state(&w).customization.drawer.is_none());
    }
    w.dispatch(HeaderAction::Edit { editing: true }.action());
    pump(300);
    assert!(
        find_named(w.window.upcast_ref(), "header-editor")
            .unwrap()
            .is_mapped()
    );
    crate::capture(&w, dir.join("editor.png").to_str().unwrap());
    let id = original.zones[0][1].id; // Menu, moved as one individual item.
    let from = point(&format!("header-grip-{id}"));
    let to = point(&format!("header-item-{}", original.zones[2][0].id));
    perform(
        serde_json::json!([{ "point": from }, { "button": 272, "down": true }, { "point": [from[0] + 18., from[1]] }, { "point": to }, { "button": 272, "down": false }]),
    );
    assert_eq!(
        state(&w).workspace.layout.header.location(id).unwrap().0,
        HeaderZone::Right
    );
    w.dispatch(HeaderAction::Cancel.action());
    w.dispatch(HeaderAction::Edit { editing: true }.action());
    pump(200);
    assert_eq!(state(&w).workspace.layout.header, original);
    let from = point(&format!("header-grip-{id}"));
    perform(
        serde_json::json!([{ "point": from }, { "button": 272, "down": true }, { "point": [from[0] + 18., from[1]] }, { "point": [800., 400.] }, { "button": 272, "down": false }]),
    );
    assert_eq!(
        state(&w).workspace.layout.header,
        original,
        "outside drop must cancel"
    );
    perform(click(point("header-edit-done")));
    assert!(!state(&w).customization.header_editing);
    assert_eq!(w.header.root.height(), 60);
    std::fs::write(dir.join("finished"), "done").unwrap();
    w.window.close();
    pump(200);
}
