//! Actual GTK widgets and Mutter-delivered input for the window-bar builder.
use super::*;

#[path = "canvas_pen_button_tests.rs"]
mod canvas_pen_buttons;
#[path = "color_picker_tests.rs"]
mod color_picker_tests;
#[path = "accent_preferences_tests.rs"]
mod accent_preferences_tests;
#[path = "palette_tests.rs"]
mod palette_tests;

fn wait_for_drawer_close(w: &Workspace) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !w.drawer.is_closed() {
        assert!(Instant::now() < deadline, "drawer close animation");
        pump(10);
    }
}

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
    fn capture_canvas(&self, name: &str) {
        // As in native_header_canvas_visual, settle the screenshot-only
        // paintable before inspecting GTK's cached whole-window scene.
        let _warm = crate::snapshot(&self.w);
        pump(120);
        let snapshot = crate::snapshot(&self.w);
        assert!(
            canvas_white(&self.w, &snapshot) > 100_000,
            "paper remains visible"
        );
        snapshot.save_to_png(self.dir.join(name)).unwrap();
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
    fn header_icon(&self, command: CommandId, icon: &str) -> gtk::Widget {
        let root = self.named(&self.header_tool(ToolbarControl::Command { command }));
        find_named(&root, &format!("layer-{icon}-symbolic"))
            .unwrap_or_else(|| panic!("{command:?} should display {icon}"))
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
            workspace: Box::new(WorkspaceState {
                layout: WorkspacePreset::Painter.layout(Platform::Gtk),
                ..WorkspaceState::default()
            }),
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
        if let Some(popup) = widget.native().and_downcast::<gtk::Popover>() {
            let b = widget.compute_bounds(&popup).unwrap();
            let surface = popup.surface().unwrap().downcast::<gdk::Popup>().unwrap();
            let (dx, dy) = popup.surface_transform();
            return [
                surface.position_x() as f32 - dx as f32 + b.x() + b.width() / 2.,
                surface.position_y() as f32 - dy as f32 + b.y() + b.height() / 2.,
            ];
        }
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
        // Native menu pages animate and can scroll at short window heights.
        // A mapped label is not necessarily inside its visible viewport.
        pump(250);
        let label = self.label(text);
        let point = self.point(&label);
        if point[1] < 0. || point[1] > self.w.window.height() as f32 {
            let mut row = label.clone();
            while !row.is_focusable() {
                row = row.parent().unwrap();
            }
            assert!(row.grab_focus());
            pump(250);
        }
        self.click(&label);
    }
    fn key(&mut self, key: u32) {
        self.perform(
            serde_json::json!([{ "key": key, "down": true }, { "key": key, "down": false }]),
        );
    }
    fn edit(&mut self) {
        // Exercise a discoverable entry, not a test-only action or shortcut.
        let item = state(&self.w)
            .workspace
            .layout
            .header
            .entries()
            .find(|e| self.named(&format!("header-item-{}", e.id)).is_mapped())
            .cloned();
        if let Some(item) = item {
            let p = self.point(&self.named(&format!("header-item-{}", item.id)));
            self.perform(serde_json::json!([{"point":p},{"button":273,"down":true},{"button":273,"down":false}]));
        } else {
            self.click_name("header-recovery");
            self.click_label("Window");
        }
        self.click_label("Customize Title Bar…");
        if !state(&self.w).customization.header_editing {
            crate::capture(&self.w, self.dir.join("failed-entry.png").to_str().unwrap());
            eprintln!(
                "Editor entry focus: {:?}",
                gtk::prelude::GtkWindowExt::focus(&self.w.window)
            );
        }
        assert!(state(&self.w).customization.header_editing);
    }
    fn drop_component(&mut self, name: &str, zone: HeaderZone) {
        let geometry = self.w.header.geometry_for_test();
        let b = geometry.zones[zone.index()];
        let point = [b.x + b.width - 2., b.y + b.height / 2.];
        self.drop_component_at(name, point, false);
    }
    fn drop_component_at(&mut self, name: &str, point: [f32; 2], touch: bool) {
        let start = self.point(&self.named(&format!("header-add-{name}")));
        self.perform(if touch {
            serde_json::json!([{"touch":"down","point":start},{"touch":"move","point":point}])
        } else {
            serde_json::json!([{"point":start},{"down":true},{"point":point}])
        });
        assert!(self.w.dragging.get(), "immediate whole-body pickup: {name}");
        assert!(
            self.w.header.drag_for_test().unwrap().action.is_some(),
            "valid drop: {name}"
        );
        self.perform(if touch {
            serde_json::json!([{"touch":"up"}])
        } else {
            serde_json::json!([{"down":false}])
        });
    }
    fn drop_component_before(&mut self, name: &str, before: u32, touch: bool) {
        let bounds = self
            .named(&format!("header-item-{before}"))
            .compute_bounds(&self.w.surface)
            .unwrap();
        self.drop_component_at(
            name,
            [bounds.x() + 2., bounds.y() + bounds.height() / 2.],
            touch,
        );
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
    assert_eq!(
        d.w.workspaces
            .manager
            .as_ref()
            .unwrap()
            .active_name()
            .as_deref(),
        Some("Paint")
    );
    for name in ["Sketch", "Paint", "Photo"] {
        d.label(name);
    }
    d.capture_canvas("paint-default.png");
    d.click_name("workspace-switch-painter");
    Driver::wait_ready(&d.w);
    pump(400);
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter_for_platform(Platform::Gtk));
    assert_eq!(
        d.w.workspaces
            .manager
            .as_ref()
            .unwrap()
            .active_name()
            .as_deref(),
        Some("Sketch")
    );
    d.capture_canvas("sketch-default.png");
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
    d.click_name("header-size-2");
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
    d.click_name("header-canvas-info");
    d.drop_component("tools", HeaderZone::Right);
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
    d.click_name("header-size-0");
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
        Some("Sketch")
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
    d.capture_canvas("managed-painter.png");
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_workspace_ownership_input --native-storage"]
fn native_workspace_ownership_input() {
    let mut d = Driver::managed("art.capycanvas.WorkspaceOwnership");
    let database = std::path::PathBuf::from(std::env::var_os("CAPY_WORKSPACE_DIR").unwrap())
        .join("workspaces.sqlite3");
    let original =
        d.w.workspaces
            .manager
            .as_ref()
            .unwrap()
            .active_id()
            .unwrap();
    let target = layer_workspace::DEFAULT_WORKSPACES
        .iter()
        .find(|(id, _)| *id != original)
        .unwrap();
    let target_id = target.0;
    let target_button = match target.1 {
        WorkspacePreset::Painter => "workspace-switch-painter",
        WorkspacePreset::Illustrator => "workspace-switch-illustrator",
        WorkspacePreset::Photographer => "workspace-switch-photographer",
    };
    let mut external = layer_workspace::SqliteStore::open(&database).unwrap();
    let owner = layer_workspace::Owner::fresh();
    let saved = external.claim(target_id, owner.clone()).unwrap();
    // A live but non-renewing owner remains protected after its timer expires.
    let output = std::process::Command::new("sqlite3")
        .arg(&database)
        .arg(format!(
            "UPDATE items SET lease_until='0' WHERE id='{target_id}'"
        ))
        .output()
        .unwrap();
    assert!(output.status.success());
    d.click_name(target_button);
    Driver::wait_ready(&d.w);
    assert_eq!(
        d.w.workspaces
            .manager
            .as_ref()
            .unwrap()
            .active_id()
            .as_deref(),
        Some(original.as_str())
    );
    assert_eq!(
        external.load(target_id).unwrap().claim.unwrap().owner,
        owner
    );
    // Keep the GTK process/worker alive while this independent owner exits.
    drop(external);
    d.click_name(target_button);
    Driver::wait_ready(&d.w);
    let manager = d.w.workspaces.manager.as_ref().unwrap();
    assert_eq!(manager.active_id().as_deref(), Some(target_id));
    assert!(manager.error().is_none());
    let mut adopted = saved.entity.working.unwrap();
    let depth =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .color
            .depth;
    adopted.colors.set_document_depth(depth).unwrap();
    adopted.colors.library.ensure_starters();
    assert_eq!(manager.current().unwrap().working, Some(adopted));

    // Context menus created while an item was occupied use SwitchToWindow.
    // That stale action must now reclaim the item too, rather than get stuck.
    let mut external = layer_workspace::SqliteStore::open(&database).unwrap();
    external
        .claim(&original, layer_workspace::Owner::fresh())
        .unwrap();
    glib::MainContext::default()
        .block_on(manager.refresh())
        .unwrap();
    drop(external);
    glib::MainContext::default()
        .block_on(d.w.workspaces.perform(
            &d.w,
            layer_workspace::ManagerAction::SwitchToWindow(original.clone()),
        ))
        .unwrap();
    Driver::wait_ready(&d.w);
    assert_eq!(manager.active_id().as_deref(), Some(original.as_str()));

    // An actual second GTK window is focused, not stolen. Closing it makes
    // its workspace immediately available to the first window.
    let second = Workspace::new(&d._app);
    second.window.present();
    Driver::wait_ready(&second);
    let second_id = second
        .workspaces
        .manager
        .as_ref()
        .unwrap()
        .active_id()
        .unwrap();
    assert_ne!(second_id, original);
    glib::MainContext::default()
        .block_on(d.w.workspaces.perform(
            &d.w,
            layer_workspace::ManagerAction::Switch(second_id.clone()),
        ))
        .unwrap();
    pump(500);
    assert_eq!(manager.active_id().as_deref(), Some(original.as_str()));
    assert!(
        second.window.is_active(),
        "switching to a live owner focuses its GTK window"
    );
    second.window.close();
    let deadline = Instant::now() + Duration::from_secs(15);
    while second.window.is_visible() {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    d.w.window.present();
    glib::MainContext::default()
        .block_on(d.w.workspaces.perform(
            &d.w,
            layer_workspace::ManagerAction::Switch(second_id.clone()),
        ))
        .unwrap();
    Driver::wait_ready(&d.w);
    assert_eq!(manager.active_id().as_deref(), Some(second_id.as_str()));
    assert!(manager.error().is_none());
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
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter_for_platform(Platform::Gtk));
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
        Some("Sketch")
    );
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter_for_platform(Platform::Gtk));
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
        Some("Sketch")
    );
    assert_eq!(
        sql("SELECT owner IS NULL FROM items WHERE id='builtin:workspace:photographer'"),
        "1"
    );
    let cancel = find_button(d.w.workspaces.ui.dialog.upcast_ref(), "Cancel").unwrap();
    d.click(cancel.upcast_ref());
    pump(250);
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter_for_platform(Platform::Gtk));
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
#[ignore = "isolated native-input.js --native-test=native_header_drag_only_bank_input"]
fn native_header_drag_only_bank_input() {
    let mut d = Driver::new("art.capycanvas.HeaderDragOnly");
    let original = state(&d.w).workspace;
    d.edit();
    fn inert(widget: &gtk::Widget) {
        assert!(!widget.is::<gtk::Button>() && !widget.is::<gtk::MenuButton>());
        assert!(
            !widget.is_focusable(),
            "bank components are not activation controls"
        );
        let mut child = widget.first_child();
        while let Some(w) = child {
            child = w.next_sibling();
            inert(&w);
        }
    }
    // Neither a tap, a hold nor sub-slop motion adds anything or opens Tools.
    for name in [
        "tools",
        "clock",
        "battery",
        "document-title",
        "menu-labels",
        "space",
    ] {
        let chip = d.named(&format!("header-component-{name}"));
        inert(&chip);
        let p = d.point(&chip);
        for touch in [false, true] {
            d.perform(if touch {
                serde_json::json!([{"touch":"down","point":p},{"touch":"move","point":[p[0]+1.,p[1]]},{"touch":"up"}])
            } else {
                serde_json::json!([{"point":p},{"down":true},{"point":[p[0]+1.,p[1]]},{"down":false}])
            });
            assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
            assert!(d.w.window.visible_dialog().is_none());
            assert!(!d.w.dragging.get() && d.w.workspace_drag.borrow().is_none());
        }
    }
    for touch in [false, true] {
        let p = d.point(&d.named("header-component-tools"));
        let mut events = if touch {
            vec![serde_json::json!({"touch":"down","point":p})]
        } else {
            vec![
                serde_json::json!({"point":p}),
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
        assert!(d.w.window.visible_dialog().is_none());
        assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
    }
    // Every visible part belongs to the same source, not just its old grip.
    for touch in [false, true] {
        for hit in 0..4 {
            let chip = d.named("header-component-clock");
            let b = chip.compute_bounds(&d.w.surface).unwrap();
            let body = d.named("header-add-clock");
            let p = match hit {
                0 => [b.x() + 2., b.y() + b.height() / 2.],
                1 => d.point(&body.first_child().unwrap()),
                2 => d.point(&body.last_child().unwrap()),
                _ => [b.x() + b.width() - 2., b.y() + b.height() - 2.],
            };
            let (source, target) = d.w.drag_source_at(p).unwrap();
            assert_eq!(source, chip);
            assert!(matches!(
                target,
                DragTarget::Header(HeaderDragSource::Component(HeaderItem::Clock))
            ));
            let to = [d.w.surface.width() as f32 / 2., 25.];
            d.perform(if touch {
                serde_json::json!([{"touch":"down","point":p},{"touch":"move","point":to}])
            } else {
                serde_json::json!([{"point":p},{"down":true},{"point":to}])
            });
            assert!(d.w.dragging.get(), "touch={touch}, hit={hit}");
            assert!(!d.w.workspace_drag.borrow().as_ref().unwrap().wait_for_hold);
            assert_eq!(
                state(&d.w).workspace.layout.header,
                original.layout.header,
                "motion stays a preview"
            );
            d.perform(if touch {
                serde_json::json!([{"touch":"up"}])
            } else {
                serde_json::json!([{"down":false}])
            });
            let model = state(&d.w).workspace.layout.header;
            let clock = model
                .entries()
                .find(|e| e.item == HeaderItem::Clock)
                .unwrap()
                .id;
            assert_eq!(model.location(clock).unwrap().0, HeaderZone::Center);
            assert!(!chip.is_mapped());
            d.click_name("header-edit-cancel");
            assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
            d.edit();
        }
    }
    crate::capture(&d.w, d.dir.join("drag-only-bank.png").to_str().unwrap());
    d.click_name("header-edit-cancel");
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
            original.entries().count() + 1,
            "touch={touch} grip={grip} held={held}"
        );
        let entry = layout
            .entries()
            .find(|e| e.item == HeaderItem::Clock)
            .unwrap();
        assert_eq!(layout.location(entry.id).unwrap().0, HeaderZone::Center);
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
    d.drop_component("tools", HeaderZone::Right);
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
    d.click_name("header-size-2");
    pump(120);
    d.click_name("header-edit-cancel");
    assert!(!state(&d.w).customization.header_editing);
    assert_eq!(state(&d.w).workspace.layout.header, preview);
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
    fn choose(d: &mut Driver, name: &str) {
        let choice = d.named(name);
        let results = choice.ancestor(gtk::ScrolledWindow::static_type()).unwrap();
        assert!(choice.grab_focus());
        let deadline = Instant::now() + Duration::from_secs(2);
        while !choice
            .compute_bounds(&results)
            .is_some_and(|b| b.y() >= 0. && b.y() + b.height() <= results.height() as f32)
        {
            assert!(Instant::now() < deadline, "{name} scrolls into view");
            pump(10);
        }
        d.click(&choice);
    }
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
        d.drop_component_before("tools", before, touch);
        assert_shared_icons(d.w.window.visible_dialog().unwrap().upcast_ref());
        assert!(d.w.window.visible_dialog().is_some());
        assert!(!d.named("toolbar-name").is_mapped());
        assert!(!d.named("confirm-tools").is_sensitive());
        search(&d, "Brush opacity");
        choose(&mut d, &opacity);
        search(&d, "Color");
        choose(&mut d, &color);
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
        d.drop_component_before("tools", before, touch);
        assert_eq!(
            d.named("tool-search")
                .downcast::<gtk::SearchEntry>()
                .unwrap()
                .text(),
            ""
        );
        assert!(!d.named("confirm-tools").is_sensitive());
        search(&d, "Brush opacity");
        choose(&mut d, &opacity);
        search(&d, "Color");
        choose(&mut d, &color);
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
        d.drop_component("clock", HeaderZone::Center);
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
        d.key(0xff08); // Backspace removes the clicked item.
        assert!(d.named("header-add-clock").is_mapped());
        // Focus/Enter does not choose an insertion slot; the actual drop does.
        let target = d.named(&format!("header-item-{before}"));
        target.grab_focus();
        d.key(0xff0d);
        d.drop_component_before("space", before, touch);
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
                let gap = if pair[0].item.joins_bar() && pair[1].item.joins_bar() {
                    size.gap()
                } else {
                    6.
                };
                assert!(
                    (b.x() - a.x() - a.width() - gap).abs() < 1.,
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
            wait_for_drawer_close(&d.w);
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
            d.drop_component("space", HeaderZone::Left);
            let model = state(&d.w).workspace.layout.header;
            let space = model
                .entries()
                .find(|e| e.item == HeaderItem::Space)
                .unwrap()
                .id;
            let space = d.named(&format!("header-item-{space}"));
            assert_eq!(
                space.width(),
                size.tile() as i32 + 20,
                "Space is one tile plus its editing grip"
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
        let mut bar = button.clone();
        while !bar.has_css_class("header-menu-labels") {
            bar = bar.parent().unwrap();
        }
        let bar_bounds = bar.compute_bounds(&d.w.surface).unwrap();
        assert_eq!(
            (bar_bounds.height(), button_bounds.height()),
            (36., 26.),
            "menu labels share the workspace switcher track at {size:?}"
        );
        let label_bounds = label.compute_bounds(&d.w.surface).unwrap();
        assert!(
            label_bounds.x() - button_bounds.x() >= 8.
                && button_bounds.x() + button_bounds.width()
                    - label_bounds.x()
                    - label_bounds.width()
                    >= 8.
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
                workspace: Box::new(WorkspaceState {
                    layout: WorkspacePreset::Painter.layout(Platform::Gtk),
                    ..WorkspaceState::default()
                }),
            });
            pump(250);
            let color = d.named(&d.header_tool(ToolbarControl::Color));
            let color_button = find_css(&color, "header-tool").unwrap();
            let brush = d.named(&d.header_tool(ToolbarControl::Command {
                command: CommandId::DrawingBrush,
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
                wait_for_drawer_close(&d.w);
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
                    workspace: Box::new(WorkspaceState {
                        layout,
                        ..WorkspaceState::default()
                    }),
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
                let grip = handle.compute_bounds(&d.w.window).unwrap();
                let exposed = [grip.x() + 6., grip.y() + grip.height() / 2.];
                assert!(
                    !d.w.drawer.placement().unwrap().bounds.contains(exposed[0], exposed[1]),
                    "toolbar handle {floating}/{touch}: the drawer covers the tapped grip"
                );
                contact(&mut d, exposed, touch);
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
    let menu_model =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .application_menu(ApplicationMenu::Window);
    assert!(!format!("{menu_model:?}").contains("Show Menu Bar"));
    d.click_label("Customize Title Bar…");
    d.drop_component("menu-labels", HeaderZone::Left);
    d.click_name("header-edit-done");
    assert!(
        state(&d.w)
            .workspace
            .layout
            .header
            .entries()
            .any(|e| e.item == HeaderItem::MenuLabels)
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
        workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Painter.layout(Platform::Gtk),
            ..WorkspaceState::default()
        }),
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
    assert!(
        !d.w.header.root.can_target(),
        "edge reveal is off by default"
    );
    let zen_capy = d.w.zen_capy.clone().upcast::<gtk::Widget>();
    d.click(&zen_capy);
    assert!(!state(&d.w).workspace.zen_mode);
    // Close is native and cannot be removed by customization.
    let close = find_css(d.w.header.root.upcast_ref(), "close").unwrap();
    d.click(&close);
    assert!(!d.w.window.is_visible());
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_brush_drawer_input"]
fn native_brush_drawer_input() {
    let mut d = Driver::new("art.capycanvas.BrushDrawer");
    let brush_icon = d.header_icon(CommandId::DrawingBrush, "pen");
    let brush = ToolbarControl::Command { command: CommandId::DrawingBrush };
    let opener = d.header_tool(brush);
    d.click_name(&opener);
    let drawer = d.named("tool-drawer");
    let sets = find_named(&drawer, "drawer-panel-BrushSets").unwrap();
    let tools = find_named(&drawer, "drawer-panel-Tools").unwrap();
    let settings = find_named(&drawer, "drawer-panel-ToolSettings").unwrap();
    assert!(sets.width() < tools.width());
    assert!(tools.width() < settings.width());
    assert!(find_css(&tools, "tool-groups").is_some_and(|w| !w.is_visible()));
    for (touch, icon, group) in [
        (false, "pencil", layer_ui::ToolGroup::Pencil),
        (true, "pastel", layer_ui::ToolGroup::Pastel),
        (false, "paint", layer_ui::ToolGroup::Paint),
    ] {
        let button = find_named(&sets, &format!("brush-set-{icon}")).unwrap();
        assert!(button.height() >= 44, "touchable brush set row");
        let p = d.point(&button);
        if touch {
            d.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
        } else { d.click(&button); }
        assert!(state(&d.w).customization.drawer.is_some());
        assert_eq!(state(&d.w).brush.tool, group.tool());
        assert_eq!(d.header_icon(CommandId::DrawingBrush, icon), brush_icon, "retain the header image while changing tools");
        assert!(state(&d.w).tool_panels.brush_sets.groups.iter().any(|g| g.selected && g.label == group.label()));
        assert_eq!(d.named("drawer-panel-BrushSets"), sets, "set list remains retained");
        let choices = state(&d.w).tool_set.subtools;
        let last = choices.last().unwrap();
        d.click_name(&format!("brush-{}", last.preview.unwrap()));
        assert_eq!(state(&d.w).brush.preset, last.preview.unwrap());
        assert!(state(&d.w).customization.drawer.is_some());
    }
    d.number(&find_named(&drawer, "tool-setting-size").unwrap(), "37");
    let remembered = state(&d.w).brush.preset;
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS")
        .unwrap_or_else(|_| d.dir.to_string_lossy().into()));
    std::fs::create_dir_all(&output).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let _warm = crate::snapshot(&d.w);
        pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("brush-drawer-{theme:?}.png"))).unwrap();
    }
    assert_eq!(state(&d.w).tool_panels.brush_sets.groups.len(), 10);
    let sculpt = d.header_tool(ToolbarControl::Command { command: CommandId::Sculpt });
    d.click_name(&sculpt);
    let sculpt_sets = d.named("drawer-panel-SculptSets");
    assert_eq!(state(&d.w).tool_panels.sculpt_sets.groups.len(), 2);
    let liquify = find_named(&sculpt_sets, "sculpt-set-liquify").unwrap();
    let p = d.point(&liquify);
    d.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
    assert_eq!(state(&d.w).brush.tool, layer_ui::Tool::Liquify);
    d.header_icon(CommandId::Sculpt, "liquify");
    assert_eq!(d.header_icon(CommandId::DrawingBrush, "paint"), brush_icon);
    assert!(state(&d.w).customization.drawer.is_some());
    d.number(&find_named(&d.named("tool-drawer"), "tool-setting-size").unwrap(), "79");
    let sculpt_preset = state(&d.w).brush.preset;
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        crate::snapshot(&d.w).save_to_png(output.join(format!("sculpt-drawer-{theme:?}.png"))).unwrap();
    }
    d.click_name(&opener);
    assert_eq!((state(&d.w).brush.preset, state(&d.w).brush.diameter), (remembered, 37.));
    d.header_icon(CommandId::Sculpt, "liquify");
    d.click_name(&sculpt);
    assert_eq!((state(&d.w).brush.preset, state(&d.w).brush.diameter), (sculpt_preset, 79.));
    d.click_name(&opener);
    d.click_name(&opener);
    assert!(state(&d.w).customization.drawer.is_none());
    d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    d.click_name(&opener);
    assert!(state(&d.w).customization.drawer.is_none(), "first click restores Brush");
    assert_eq!(state(&d.w).brush.preset, remembered);
    assert_eq!(state(&d.w).brush.diameter, 37.);
    d.click_name(&opener);
    assert!(state(&d.w).customization.drawer.is_some());
    assert_eq!(d.w.gpu.borrow().as_ref().unwrap().session.command(CommandId::Brush).label, "Paint Brush");
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_filter_drawer_input"]
fn native_filter_drawer_input() {
    let mut d = Driver::new("art.capycanvas.FilterDrawer");
    let opener = d.header_tool(ToolbarControl::Panel { panel: Panel::Adjustments });
    d.click_name(&opener);
    let types = d.named("drawer-panel-FilterTypes");
    let choices = d.named("drawer-panel-Adjustments");
    assert!(types.width() < choices.width());
    assert!(find_css(&choices, "filter-picker-header").is_some_and(|w| !w.is_visible()));
    d.click_name("adjustment-brightness_contrast");
    let id = state(&d.w).layer_properties.layer.unwrap();
    assert!(state(&d.w).layers.iter().any(|l| l.id == 1 && l.drawing));
    d.number(&d.named("property-brightness"), "20");
    assert!(state(&d.w).customization.drawer.is_some());
    d.click_name(&opener);
    d.click_name(&opener);
    assert_eq!(state(&d.w).layer_properties.layer, Some(id));
    d.click_name("adjustment-curves");
    assert_eq!(state(&d.w).layer_properties.layer, Some(id));
    assert_eq!(state(&d.w).layers.len(), 3);
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let _warm = crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("filters-{theme:?}.png"))).unwrap();
    }
    d.click_name("cancel-filter");
    assert_eq!(state(&d.w).layers.len(), 2);
    assert!(state(&d.w).customization.drawer.is_none());
    d.click_name(&opener);
    d.w.dispatch(UiAction::SelectLayer { id: 2 });
    d.w.dispatch(UiAction::SetColor { rgba: [0.08, 0.1, 0.15, 1.] });
    pump(100);
    d.click_name("paper-color-bucket");
    assert!(state(&d.w).layer_properties.controls[0].color_action.is_some());
    let _warm = crate::snapshot(&d.w); pump(120);
    crate::snapshot(&d.w).save_to_png(output.join("paper-properties.png")).unwrap();
    // Native hover exercises the GPU's prohibited cursor path.
    d.click_name(&opener);
    d.perform(serde_json::json!([{"point":[600.,400.]}]));
    assert_eq!(d.w.gpu.borrow_mut().as_mut().unwrap().session.canvas_cursor().unwrap().segments[0].marker, 6.);
    d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::New { group: false, clipped: false } });
    let removable = state(&d.w).layer_properties.layer.unwrap();
    let layers = d.header_tool(ToolbarControl::Panel { panel: Panel::Layers });
    d.click_name(&layers);
    let row = d.named(&format!("art-layer-{removable}"));
    let swipe = row.parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
    let before = row.compute_bounds(&d.w.window).unwrap().x();
    let p = d.point(&row);
    d.perform(serde_json::json!([{"touch":"down","point":p},
        {"touch":"move","point":[p[0]-18.,p[1]]}, {"touch":"move","point":[p[0]-48.,p[1]]}]));
    assert!(swipe.is_open());
    let moved = before - row.compute_bounds(&d.w.window).unwrap().x();
    assert!((moved - 48.).abs() < 2., "row follows the finger: {moved}");
    d.perform(serde_json::json!([{"touch":"up"}]));
    let _warm = crate::snapshot(&d.w); pump(120);
    crate::snapshot(&d.w).save_to_png(output.join("layer-delete.png")).unwrap();
    let p = d.point(&row);
    d.perform(serde_json::json!([{"touch":"down","point":p},
        {"touch":"move","point":[p[0]+18.,p[1]]}, {"touch":"move","point":[p[0]+65.,p[1]]}, {"touch":"up"}]));
    assert!(!swipe.is_open(), "reverse swipe closes Delete");
    for delete in [false, true] {
        let row = d.named(&format!("art-layer-{removable}"));
        let swipe = row.parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        let p = d.point(&row);
        d.perform(serde_json::json!([{"touch":"down","point":p},
            {"touch":"move","point":[p[0]-18.,p[1]]}, {"touch":"move","point":[p[0]-65.,p[1]]}, {"touch":"up"}]));
        assert!(swipe.is_open());
        if delete { d.click(swipe.delete_button().upcast_ref()); }
        else {
            d.perform(serde_json::json!([{"point":[600.,400.]},{"down":true},{"down":false}]));
            assert!(!swipe.is_open(), "outside click closes Delete");
            if state(&d.w).customization.drawer.is_none() { d.click_name(&layers); }
        }
    }
    assert!(!state(&d.w).layers.iter().any(|l| l.id == removable));
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    assert!(state(&d.w).layers.iter().any(|l| l.id == removable));
    d.w.window.close();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_layer_preview_selection"]
fn native_layer_preview_selection() {
    let mut d = Driver::new("art.capycanvas.LayerPreviewSelection");
    d.w.dispatch(UiAction::SetColor { rgba: [0., 1., 0., 1.] });
    d.w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    pump(200);
    d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::FillSelection });
    pump(300);
    d.w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    let opener = d.header_tool(ToolbarControl::Panel { panel: Panel::Layers });
    d.click_name(&opener);
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());
    super::place_source::wait_layer_thumbnail(&d.w, 1);
    for id in [2, 1, 2, 1] {
        let row = d.named(&format!("art-layer-{id}"));
        d.click(&find_css(&row, "layer-thumbnail").unwrap());
        super::place_source::wait_layer_thumbnail(&d.w, 1);
        let _warm = crate::snapshot(&d.w); pump(120);
        let shot = crate::snapshot(&d.w);
        let row = d.named("art-layer-1");
        let thumb = find_css(&row, "layer-thumbnail").unwrap();
        let p = thumb.compute_bounds(&d.w.surface).unwrap();
        let mut bytes = vec![0; shot.width() as usize * shot.height() as usize * 4];
        shot.download(&mut bytes, shot.width() as usize * 4);
        let x = (p.x()+p.width()*0.5) as usize;
        let y = (p.y()+p.height()*0.5) as usize;
        let pixel = &bytes[(y*shot.width() as usize+x)*4..][..4];
        shot.save_to_png(output.join(format!("thumbnail-selected-{id}.png"))).unwrap();
        assert!(pixel[1] > 180 && pixel[0] < 80 && pixel[2] < 80, "selected {id}, thumbnail center {pixel:?}");
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_panel_pen_input --tablet"]
fn native_panel_pen_input() {
    let mut d = Driver::new("art.capycanvas.PanelPen");
    let opener = d.header_tool(ToolbarControl::Command { command: CommandId::DrawingBrush });
    d.click_name(&opener);
    let sets = state(&d.w).tool_panels.brush_sets.groups;
    let mut longest = (0, sets[0].action.clone());
    for set in sets {
        d.w.dispatch(set.action.clone());
        let count = state(&d.w).tool_set.subtools.len();
        if count > longest.0 { longest = (count, set.action); }
    }
    d.w.dispatch(longest.1);
    pump(200);
    let tools = d.named("drawer-panel-Tools");
    let scroll = tools.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
    // Constrain this native viewport to make every device exercise overflow.
    scroll.set_max_content_height(180);
    scroll.set_propagate_natural_height(false);
    scroll.set_height_request(180);
    scroll.set_valign(gtk::Align::Start);
    pump(200);
    let overflow = scroll.vadjustment().upper() - scroll.vadjustment().page_size();
    assert!(overflow > 20., "fixture has scrollable tools");
    let bounds = scroll.compute_bounds(&d.w.window).unwrap();
    let p = [bounds.x() + bounds.width() * 0.5, bounds.y() + bounds.height().min(160.) - 15.];
    let input_tools = Rc::new(Cell::new(0));
    let events = gtk::EventControllerLegacy::new();
    events.set_propagation_phase(gtk::PropagationPhase::Capture);
    events.connect_event(glib::clone!(#[strong] input_tools, move |_, event| {
        if event.device_tool().is_some() { input_tools.set(input_tools.get() + 1); }
        glib::Propagation::Proceed
    }));
    tools.add_controller(events);
    for device in ["mouse", "touch", "pen"] {
        scroll.vadjustment().set_value(0.);
        pump(80);
        let mut input = Vec::new();
        if device == "mouse" { input.extend([serde_json::json!({"point":p}), serde_json::json!({"down":true})]); }
        else { input.push(serde_json::json!({device:"down","point":p})); }
        for delta in [18., 40., 65., 95.] {
            let q = [p[0],p[1]-delta];
            input.push(if device == "mouse" { serde_json::json!({"point":q}) } else { serde_json::json!({device:"move","point":q}) });
        }
        input.push(if device == "mouse" { serde_json::json!({"down":false}) } else { serde_json::json!({device:"up"}) });
        d.perform(input.into());
        if device == "mouse" { assert_eq!(scroll.vadjustment().value(), 0., "mouse choices do not pan"); }
        else { assert!(scroll.vadjustment().value() > 40_f64.min(overflow * 0.8), "{device} scrolls choices: {}", scroll.vadjustment().value()); }
    }
    assert!(input_tools.get() > 0, "Wayland tablet-v2 produced real GDK tablet events");
    d.perform(serde_json::json!([{"pen":"leave"}]));
    d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::New { group: false, clipped: false } });
    let id = state(&d.w).layer_properties.layer.unwrap();
    let layers = d.header_tool(ToolbarControl::Panel { panel: Panel::Layers });
    d.click_name(&layers);
    let row = d.named(&format!("art-layer-{id}"));
    let swipe = row.parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
    let p = d.point(&row);
    d.perform(serde_json::json!([{"pen":"down","point":p}, {"pen":"move","point":[p[0]-20.,p[1]]},
        {"pen":"move","point":[p[0]-64.,p[1]]},{"pen":"up"}]));
    assert!(swipe.is_open(), "pen swipes expose Delete");
    let p = d.point(swipe.delete_button().upcast_ref());
    d.perform(serde_json::json!([{"pen":"down","point":p},{"pen":"up"},{"pen":"leave"}]));
    assert!(!state(&d.w).layers.iter().any(|l| l.id == id));
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_drawer_controls_input"]
fn native_header_drawer_controls_input() {
    let mut d = Driver::new("art.capycanvas.HeaderDrawers");
    let brush = ToolbarControl::Command {
        command: CommandId::DrawingBrush,
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
        command: CommandId::Sculpt,
    }));
    assert!(state(&d.w).customization.drawer.is_some());
    assert!(
        tool_state(
            &state(&d.w),
            ToolbarControl::Command {
                command: CommandId::Sculpt
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
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(workspace),
        });
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
                    workspace: Box::new(original.clone()),
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
                    HeaderZone::Center,
                    "touch={touch} grip={grip} held={held}"
                );
                d.w.dispatch(HeaderAction::Cancel.action());
                pump(160);
                assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
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
    d.click_label("Customize Title Bar…");
    let p = d.point(&d.named(&format!("header-item-{id}")));
    d.perform(serde_json::json!([{ "point": p }, { "button": 273, "down": true }, { "button": 273, "down": false }]));
    d.click_label("Move to Center");
    assert_eq!(
        state(&d.w).workspace.layout.header.location(id).unwrap().0,
        HeaderZone::Center
    );
    let item = d.named(&format!("header-item-{id}"));
    item.grab_focus();
    d.perform(serde_json::json!([{ "key": 0xffe1, "down": true }, { "key": 0xffc7, "down": true }, { "key": 0xffc7, "down": false }, { "key": 0xffe1, "down": false }]));
    d.click_label("Remove from Title Bar");
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
        d.click_name(&format!("header-size-{}", size as usize));
        pump(220);
        assert_eq!(state(&d.w).workspace.layout.header.size, size);
        let editor = d.named("header-editor");
        assert!(
            editor.height() <= 56 && editor.width() == d.w.surface.width() - 12,
            "The editor is one full-width strip at {size:?}: {}x{}",
            editor.width(),
            editor.height()
        );
        let palette = d.named("header-add-tools").compute_bounds(&editor).unwrap();
        let done = d.named("header-edit-done").compute_bounds(&editor).unwrap();
        assert!((palette.y() + palette.height() / 2. - done.y() - done.height() / 2.).abs() < 1.);
        assert!(done.x() + done.width() >= editor.width() as f32 - 13.);
        assert!(d.w.header.root.height() <= size.height() as i32 + 68);
        assert_eq!(d.w.area.height(), d.w.surface.height());
    }
    for name in ["clock", "menu-labels"] {
        d.drop_component(name, HeaderZone::Left);
    }
    d.drop_component("tools", HeaderZone::Left);
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
    d.click_name("header-canvas-info");
    assert_eq!(
        d.named("header-canvas-info")
            .downcast::<gtk::CheckButton>()
            .unwrap()
            .label()
            .as_deref(),
        Some("Show footer")
    );
    assert!(state(&d.w).workspace.layout.canvas_info.visible && d.w.view_info.is_visible());
    assert!(d.w.resolved().status.y > d.w.surface.height() as f32 / 2.);
    assert_eq!(d.w.view_info.halign(), gtk::Align::End);
    d.click_name("header-edit-cancel");
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter_for_platform(Platform::Gtk));
    d.edit();
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
    d.click_label("Customize Title Bar…");
    assert!(state(&d.w).customization.header_editing);
    crate::capture(&d.w, d.dir.join("empty-bar-palette.png").to_str().unwrap());
    for (i, item) in HeaderItem::COMPONENTS
        .into_iter()
        .filter(|item| item.available_on(Platform::Gtk))
        .enumerate()
    {
        let zone = HeaderZone::ALL[i % 3];
        let name = item.label().to_lowercase().replace(' ', "-");
        d.drop_component(&name, zone);
        let model = state(&d.w).workspace.layout.header;
        let id = model.entries().find(|e| e.item == item).unwrap().id;
        assert_eq!(model.location(id).unwrap().0, zone);
        assert_eq!(
            d.named(&format!("header-add-{name}")).is_mapped(),
            !item.singleton()
        );
    }
    crate::capture(
        &d.w,
        d.dir.join("all-components-added.png").to_str().unwrap(),
    );
    d.click_name("header-edit-done");
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_editor_keyboard_input"]
fn native_header_editor_keyboard_input() {
    let mut d = Driver::new("art.capycanvas.HeaderKeyboard");
    let original = state(&d.w).workspace.layout;
    d.edit();
    assert_eq!(
        gtk::prelude::GtkWindowExt::focus(&d.w.window)
            .unwrap()
            .widget_name(),
        "header-size-1"
    );
    for absent in [
        "header-help",
        "header-reset",
        "header-remove-item",
        "header-zone-0",
        "header-move-earlier",
    ] {
        assert!(find_named(d.w.window.upcast_ref(), absent).is_none());
    }
    let id = original.header.zones[0][1].id;
    d.click_name(&format!("header-item-{id}"));
    for forward in [
        true, true, true, true, true, true, false, false, false, false, false, false,
    ] {
        let previous = state(&d.w).workspace.layout.header;
        let expected = previous.step(id, forward).unwrap();
        let HeaderAction::Move { zone, before, .. } = expected else {
            panic!()
        };
        let mut next = previous.clone();
        next.move_item(id, zone, before).unwrap();
        d.key(if forward { 0xff53 } else { 0xff51 });
        assert_eq!(state(&d.w).workspace.layout.header, next);
        assert_eq!(
            gtk::prelude::GtkWindowExt::focus(&d.w.window)
                .unwrap()
                .widget_name(),
            format!("header-item-{id}")
        );
    }
    assert_eq!(state(&d.w).workspace.layout.header, original.header);
    // Existing native keyboard context action and focus return survive rebuild.
    d.key(0xff67);
    d.click_label("Move to Center");
    assert_eq!(
        state(&d.w).workspace.layout.header.location(id).unwrap().0,
        HeaderZone::Center
    );
    d.key(0xff08);
    assert!(state(&d.w).workspace.layout.header.entry(id).is_err());
    let next = gtk::prelude::GtkWindowExt::focus(&d.w.window)
        .unwrap()
        .widget_name()
        .strip_prefix("header-item-")
        .unwrap()
        .parse::<u32>()
        .unwrap();
    d.key(0xffff);
    assert!(state(&d.w).workspace.layout.header.entry(next).is_err());
    assert!(d.named("header-add-main-menu").is_mapped());
    // Native Tab traverses controls and never fires canvas shortcuts.
    let mut sized = false;
    for _ in 0..36 {
        d.key(0xff09);
        let focus = gtk::prelude::GtkWindowExt::focus(&d.w.window).unwrap();
        assert!(focus == d.w.header.root || focus.is_ancestor(&d.w.header.root));
        assert!(!state(&d.w).workspace.zen_mode);
        if focus.widget_name() == "header-size-2" {
            d.key(0x20);
            assert_eq!(state(&d.w).workspace.layout.header.size, HeaderSize::Large);
            sized = true;
        }
    }
    assert!(sized);
    d.click_name("header-edit-cancel");
    assert_eq!(state(&d.w).workspace.layout.header, original.header);
    assert_eq!(
        state(&d.w).workspace.layout.canvas_info,
        original.canvas_info
    );
    d.edit();
    let root = d.w.header.root.clone();
    let x = (12..root.width() - 72)
        .step_by(12)
        .find(|x| {
            root.pick(*x as f64, 20., gtk::PickFlags::DEFAULT).as_ref() == Some(root.upcast_ref())
        })
        .unwrap();
    d.perform(serde_json::json!([{"point":[x,20]},{"button":273,"down":true},{"button":273,"down":false}]));
    d.click_label("Cancel Changes");
    assert!(!state(&d.w).customization.header_editing);
    d.finish();
}

#[test]
#[ignore = "640x480 isolated compositor, --native-test=native_header_editor_short_window_input"]
fn native_header_editor_short_window_input() {
    let mut d = Driver::new("art.capycanvas.HeaderShortWindow");
    assert!(
        d.w.surface.height() == 480,
        "run at the app’s minimum size with LAYER_MOTION_VIEWPORT=640x480"
    );
    d.edit();
    d.w.dispatch(
        HeaderAction::SetSize {
            size: HeaderSize::Large,
        }
        .action(),
    );
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
    let panel = d
        .named("header-editor")
        .downcast::<gtk::ScrolledWindow>()
        .unwrap();
    let bounds = panel.compute_bounds(&d.w.surface).unwrap();
    assert!(bounds.y() + bounds.height() <= d.w.surface.height() as f32);
    assert_eq!(panel.width(), d.w.surface.width() - 12);
    assert!(
        panel.height() <= 200,
        "full palette uses the available width"
    );
    // The simplified palette can fit without scrolling even at the minimum
    // size. Either way, native focus must reveal the footer, never clip it.
    assert!(panel.height() <= 480 - HeaderSize::Large.height() as i32 - 12);
    for _ in 0..40 {
        if gtk::prelude::GtkWindowExt::focus(&d.w.window)
            .unwrap()
            .widget_name()
            == "header-edit-done"
        {
            break;
        }
        d.key(0xff09);
    }
    let focus = gtk::prelude::GtkWindowExt::focus(&d.w.window).unwrap();
    assert_eq!(focus.widget_name(), "header-edit-done");
    let done = focus.compute_bounds(&d.w.surface).unwrap();
    assert!(done.y() >= bounds.y() && done.y() + done.height() <= bounds.y() + bounds.height());
    crate::capture(&d.w, d.dir.join("short-editor-done.png").to_str().unwrap());
    d.key(0xff0d);
    assert!(!state(&d.w).customization.header_editing);
    d.edit();
    d.click_name("header-edit-cancel");
    assert!(!state(&d.w).customization.header_editing);
    d.finish();
}

#[test]
#[ignore = "640px isolated compositor, --native-test=native_header_compact_switcher_input --native-storage"]
fn native_header_compact_switcher_input() {
    let mut d = Driver::managed("art.capycanvas.HeaderCompactSwitcher");
    assert!(
        d.w.surface.width() <= 800,
        "use LAYER_MOTION_VIEWPORT=640x600"
    );
    let manager = d.w.workspaces.manager.as_ref().unwrap().clone();
    glib::MainContext::default().block_on(async {
        let wide = manager
            .create_workspace(
                "Wide workspace",
                None,
                false,
                crate::workspace::manager::now_ms(),
            )
            .await
            .unwrap();
        let id = wide.entity.id.clone();
        d.w.workspaces.adopt(&d.w, Ok(wide)).await;
        manager
            .edit_switcher(layer_workspace::SwitcherEdit::Show { id, visible: true })
            .await
            .unwrap();
    });
    Driver::wait_ready(&d.w);
    let ids = manager.switcher_display_ids();
    assert_eq!(ids.len(), 4);
    glib::MainContext::default()
        .block_on(manager.edit_switcher(layer_workspace::SwitcherEdit::Move {
            id: ids[2].clone(),
            before: Some(ids[0].clone()),
        }))
        .unwrap();
    d.w.workspaces.update_status();

    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for (input, size) in HeaderSize::ALL.into_iter().enumerate() {
            let mut workspace = state(&d.w).workspace;
            workspace.layout.header = HeaderLayout::painter();
            workspace.layout.header.size = size;
            d.w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(workspace),
            });
            pump(250);
            let selector = d.named("header-workspace-selector");
            assert!(selector.is_mapped(), "compact selector at {size:?}");
            assert_eq!(
                selector.downcast_ref::<gtk::MenuButton>().unwrap().label(),
                manager.active_name().map(Into::into)
            );
            let mut overflow_count = 0;
            for zone in HeaderZone::ALL {
                let button = d.named(&format!("header-overflow-{}", zone.index()));
                if button.is_mapped() {
                    overflow_count += 1;
                    let image = button
                        .downcast_ref::<gtk::MenuButton>()
                        .unwrap()
                        .child()
                        .and_downcast::<gtk::Image>()
                        .unwrap();
                    assert_eq!(image.pixel_size(), size.icon());
                    assert_eq!(
                        image.measure(gtk::Orientation::Horizontal, -1).1,
                        size.icon()
                    );
                    assert_eq!(image.measure(gtk::Orientation::Vertical, -1).1, size.icon());
                    let bounds = image.compute_bounds(&button).unwrap();
                    assert!(bounds.width() >= size.icon() as f32);
                    assert!(bounds.height() >= size.icon() as f32);
                    assert!(
                        (bounds.x() + bounds.width() / 2. - button.width() as f32 / 2.).abs() < 1.
                    );
                    assert!(
                        (bounds.y() + bounds.height() / 2. - button.height() as f32 / 2.).abs()
                            < 1.
                    );
                }
            }
            assert!(overflow_count > 0);
            d.click(&selector);
            let popup = d.named("workspace-switcher-popup");
            let model = popup
                .downcast_ref::<gtk::PopoverMenu>()
                .unwrap()
                .menu_model()
                .unwrap();
            let choices = manager.switcher_display_ids();
            assert_eq!(model.n_items() as usize, choices.len());
            for (index, id) in choices.iter().enumerate() {
                assert_eq!(
                    model
                        .item_attribute_value(index as i32, "target", None)
                        .unwrap()
                        .get::<String>()
                        .as_ref(),
                    Some(id),
                    "dropdown follows the pill's configured order"
                );
            }
            crate::capture(
                &d.w,
                d.dir
                    .join(format!("compact-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            capture_popover(
                popup
                    .downcast_ref::<gtk::PopoverMenu>()
                    .unwrap()
                    .upcast_ref(),
                d.dir
                    .join(format!("choices-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            let target = choices
                .iter()
                .find(|id| Some(*id) != manager.active_id().as_ref())
                .unwrap()
                .clone();
            let name = manager
                .items()
                .into_iter()
                .find(|item| item.id == target)
                .unwrap()
                .metadata
                .name;
            let label = d.label(&name);
            match input {
                0 => d.click(&label),
                1 => {
                    let point = d.point(&label);
                    d.perform(serde_json::json!([
                        { "touch": "down", "point": point }, { "touch": "up" }
                    ]));
                }
                _ => {
                    let mut row = label;
                    while !row.is_focusable() {
                        row = row.parent().unwrap();
                    }
                    assert!(row.grab_focus());
                    d.key(0xff0d);
                }
            }
            Driver::wait_ready(&d.w);
            assert_eq!(manager.active_id().as_ref(), Some(&target));
            assert!(!popup.is_visible());
        }
    }

    // A switcher moved into a crowded zone uses the same choices from overflow.
    let mut workspace = state(&d.w).workspace;
    workspace.layout.header = HeaderLayout::painter();
    workspace.layout.header.size = HeaderSize::Large;
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    let id = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .find(|entry| entry.item == HeaderItem::Workspaces)
        .unwrap()
        .id;
    d.w.dispatch(
        HeaderAction::Move {
            id,
            zone: HeaderZone::Right,
            before: None,
        }
        .action(),
    );
    pump(250);
    assert!(!d.named(&format!("header-item-{id}")).is_mapped());
    d.click_name("header-overflow-2");
    d.click_name(&format!("header-overflow-item-{id}"));
    assert!(d.named("workspace-switcher-popup").is_mapped());
    let target = manager
        .switcher_display_ids()
        .into_iter()
        .find(|id| Some(id) != manager.active_id().as_ref())
        .unwrap();
    let name = manager
        .items()
        .into_iter()
        .find(|item| item.id == target)
        .unwrap()
        .metadata
        .name;
    d.click_label(&name);
    Driver::wait_ready(&d.w);
    assert_eq!(manager.active_id().as_ref(), Some(&target));
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
                    if find_named(
                        d.w.window.upcast_ref(),
                        &format!("header-overflow-item-{}", entry.id),
                    )
                    .is_none()
                    {
                        crate::capture(&d.w, d.dir.join("missing-overflow.png").to_str().unwrap());
                        eprintln!(
                            "Missing overflow {:?} at {:?}; dialog {:?} fullscreen {}",
                            entry.item,
                            size,
                            d.w.window.visible_dialog(),
                            d.w.window.is_fullscreen()
                        );
                    }
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
                    if entry.item == HeaderItem::Settings {
                        // On a narrow NavigationSplitView, Escape navigates
                        // back to the sidebar first. Use the native close control.
                        let content =
                            find_named(d.w.preferences.dialog.upcast_ref(), "preferences-content")
                                .unwrap();
                        d.click(&find_css(&content, "close").unwrap());
                    } else {
                        d.key(0xff1b);
                    }
                    pump(400);
                    if entry.item == HeaderItem::Fullscreen && d.w.window.is_fullscreen() {
                        // Keep each overflow case in the same window geometry.
                        d.key(0xffc8); // F11, native Full Screen shortcut.
                        pump(400);
                    }
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
            d.drop_component("tools", HeaderZone::Center);
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
        workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Painter.layout(Platform::Gtk),
            ..WorkspaceState::default()
        }),
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
            command: CommandId::Sculpt,
        },
        ToolbarControl::Command {
            command: CommandId::DrawingBrush,
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
    assert!(
        state(&w).workspace.layout.header.entry(id).is_err(),
        "outside drop removes the detached item"
    );
    perform(click(point("header-edit-done")));
    assert!(!state(&w).customization.header_editing);
    assert_eq!(w.header.root.height(), 60);
    std::fs::write(dir.join("finished"), "done").unwrap();
    w.window.close();
    pump(200);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_slide_remove_input"]
fn native_header_slide_remove_input() {
    let mut d = Driver::new("art.capycanvas.HeaderSlideRemove");
    let original = state(&d.w).workspace.layout.header;
    for touch in [false, true] {
        for tool in [false, true] {
            d.edit();
            let id = original.zones[0][if tool { 2 } else { 1 }].id;
            let neighbor = original.zones[0][if tool { 3 } else { 2 }].id;
            let widget = d.named(&format!("header-item-{id}"));
            let rect = widget.compute_bounds(&d.w.surface).unwrap();
            let start = [rect.x() + rect.width() - 8., rect.y() + rect.height() / 2.];
            let offset = [start[0] - rect.x(), start[1] - rect.y()];
            let initial = d.w.header.geometry_for_test();
            let move_to = |d: &mut Driver, point: [f32; 2]| {
                d.perform(if touch {
                    serde_json::json!([{"touch":"move","point":point}])
                } else {
                    serde_json::json!([{"point":point}])
                })
            };
            d.perform(if touch {
                serde_json::json!([{"touch":"down","point":start}])
            } else {
                serde_json::json!([{"point":start},{"down":true}])
            });
            move_to(&mut d, [start[0] + 50., start[1] + 4.]);
            let preview = d.w.header.drag_for_test().expect("native drag visual");
            assert!(!preview.detached);
            assert!((preview.held.x - (rect.x() + 50.)).abs() < 0.1);
            assert_eq!(preview.held.y, 6.);
            let x =
                |g: &HeaderGeometry| g.items.iter().find(|m| m.id == neighbor).unwrap().bounds.x;
            assert!(
                x(&preview.geometry) < x(&initial),
                "neighbors shift before release"
            );
            assert_eq!(
                state(&d.w).workspace.layout.header,
                original,
                "motion is only a preview"
            );
            crate::capture(
                &d.w,
                d.dir
                    .join(format!("sliding-{touch}-{tool}.png"))
                    .to_str()
                    .unwrap(),
            );
            // Backtracking uses grab-time slots, not the animated rectangles.
            move_to(&mut d, start);
            assert_eq!(
                d.w.header.drag_for_test().unwrap().geometry.items,
                initial.items
            );
            let outside = [start[0] + 170., 240.];
            move_to(&mut d, outside);
            let preview = d.w.header.drag_for_test().unwrap();
            assert!(preview.detached && preview.target.is_none());
            assert_eq!(preview.held.x, outside[0] - offset[0]);
            assert_eq!(preview.held.y, outside[1] - offset[1]);
            assert!(
                matches!(preview.action, Some(HeaderAction::Remove { id:removed }) if removed == id)
            );
            crate::capture(
                &d.w,
                d.dir
                    .join(format!("detached-{touch}-{tool}.png"))
                    .to_str()
                    .unwrap(),
            );
            // Re-entering attaches the same contact without applying an edit.
            let center = [d.w.surface.width() as f32 / 2., start[1]];
            move_to(&mut d, center);
            assert!(!d.w.header.drag_for_test().unwrap().detached);
            assert_eq!(
                d.w.header.drag_for_test().unwrap().target.unwrap().0,
                HeaderZone::Center
            );
            move_to(&mut d, outside);
            d.perform(if touch {
                serde_json::json!([{"touch":"up"}])
            } else {
                serde_json::json!([{"down":false}])
            });
            assert!(state(&d.w).workspace.layout.header.entry(id).is_err());
            assert!(d.w.header.drag_for_test().is_none());
            for entry in state(&d.w).workspace.layout.header.entries() {
                assert_eq!(d.named(&format!("header-item-{}", entry.id)).opacity(), 1.);
            }
            assert_eq!(
                d.named("header-add-main-menu").is_mapped(),
                !tool,
                "only removed singleton components return to the palette"
            );
            d.click_name("header-edit-cancel");
            assert_eq!(state(&d.w).workspace.layout.header, original);
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_tools_drop_input"]
fn native_header_tools_drop_input() {
    let mut d = Driver::new("art.capycanvas.HeaderToolsDrop");
    let original = state(&d.w).workspace.layout.header;
    for touch in [false, true] {
        d.edit();
        let start = d.point(&d.named("header-add-tools"));
        let target = [d.w.surface.width() as f32 / 2., 25.];
        d.perform(if touch {
            serde_json::json!([{"touch":"down","point":start},{"touch":"move","point":target}])
        } else {
            serde_json::json!([{"point":start},{"down":true},{"point":target}])
        });
        assert!(matches!(
            d.w.header.drag_for_test().unwrap().action,
            Some(HeaderAction::InsertTools {
                zone: HeaderZone::Center,
                ..
            })
        ));
        assert!(
            d.w.window.visible_dialog().is_none(),
            "picker opens on drop, not during drag"
        );
        crate::capture(
            &d.w,
            d.dir
                .join(format!("tools-drop-{touch}.png"))
                .to_str()
                .unwrap(),
        );
        d.perform(if touch {
            serde_json::json!([{"touch":"up"}])
        } else {
            serde_json::json!([{"down":false}])
        });
        assert!(d.w.window.visible_dialog().is_some());
        assert!(d.w.header.drag_for_test().is_none());
        assert_eq!(
            state(&d.w).workspace.layout.header,
            original,
            "a Tools placeholder is never stored"
        );
        // Native editing keys belong to search, not to the selected bar item.
        d.key(0x61);
        d.key(0xff08);
        d.key(0xffff);
        assert_eq!(state(&d.w).workspace.layout.header, original);
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
        let model = state(&d.w).workspace.layout.header;
        let entry = model
            .entries()
            .find(|e| {
                e.item
                    == HeaderItem::Tool {
                        control: ToolbarControl::Opacity,
                    }
            })
            .unwrap();
        assert_eq!(model.location(entry.id).unwrap().0, HeaderZone::Center);
        d.click_name("header-edit-cancel");
        assert_eq!(state(&d.w).workspace.layout.header, original);
    }
    d.finish();
}

#[test]
#[ignore = "640px isolated compositor, --native-test=native_header_overflow_drag_input"]
fn native_header_overflow_drag_input() {
    let mut d = Driver::new("art.capycanvas.HeaderOverflowDrag");
    assert!(d.w.surface.width() <= 800, "run at 640x600");
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(250);
        for touch in [false, true] {
            d.edit();
            let original = state(&d.w).workspace.layout.header;
            let geometry = d.w.header.geometry_for_test();
            assert!(geometry.overflow.iter().any(Option::is_some));
            let id = geometry.items[0].id;
            let start = d.point(&d.named(&format!("header-grip-{id}")));
            let outside = [start[0] + 30., size.height() + 180.];
            d.perform(if touch {
                serde_json::json!([{"touch":"down","point":start},{"touch":"move","point":outside}])
            } else {
                serde_json::json!([{"point":start},{"down":true},{"point":outside}])
            });
            let preview = d.w.header.drag_for_test().expect("overflow drag preview");
            assert!(preview.detached);
            assert_eq!(state(&d.w).workspace.layout.header, original);
            crate::capture(
                &d.w,
                d.dir
                    .join(format!("overflow-drag-{size:?}-{touch}.png"))
                    .to_str()
                    .unwrap(),
            );
            d.perform(if touch {
                serde_json::json!([{"touch":"up"}])
            } else {
                serde_json::json!([{"down":false}])
            });
            assert!(d.w.header.drag_for_test().is_none());
            let model = state(&d.w).workspace.layout.header;
            assert_eq!(model.entries().count(), original.entries().count() - 1);
            assert!(model.entry(id).is_err());
            assert_eq!(d.w.header.geometry_for_test().items, preview.geometry.items);
            for entry in model.entries() {
                assert_eq!(d.named(&format!("header-item-{}", entry.id)).opacity(), 1.);
            }
            // The editor transaction restores hidden entries as well as visible
            // ones. No temporary capture allocation becomes authoritative.
            d.click_name("header-edit-cancel");
            assert_eq!(state(&d.w).workspace.layout.header, original);
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_empty_center_input"]
fn native_header_empty_center_input() {
    let mut d = Driver::new("art.capycanvas.HeaderEmptyCenter");
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(250);
        for touch in [false, true] {
            for edge in [-1., 1.] {
                d.edit();
                let original = state(&d.w).workspace.layout.header;
                let id = original.zones[1][0].id;
                d.click_name(&format!("header-item-{id}"));
                d.key(0xffff);
                assert!(state(&d.w).workspace.layout.header.zones[1].is_empty());
                let b = d.w.header.geometry_for_test().zones[1];
                assert!(b.width >= 160., "empty center remains easy to target");
                let point = [
                    b.x + b.width / 2. + edge * (b.width / 2. - 10.),
                    b.y + b.height / 2.,
                ];
                assert!((point[0] - d.w.surface.width() as f32 / 2.).abs() > 40.);
                let start = d.point(&d.named("header-add-grip-workspace-switcher"));
                d.perform(if touch {
                    serde_json::json!([{"touch":"down","point":start},{"touch":"move","point":point}])
                } else {
                    serde_json::json!([{"point":start},{"down":true},{"point":point}])
                });
                assert_eq!(
                    d.w.header.drag_for_test().unwrap().target,
                    Some((HeaderZone::Center, None))
                );
                crate::capture(
                    &d.w,
                    d.dir
                        .join(format!("empty-center-{size:?}-{touch}-{edge}.png"))
                        .to_str()
                        .unwrap(),
                );
                d.perform(if touch {
                    serde_json::json!([{"touch":"up"}])
                } else {
                    serde_json::json!([{"down":false}])
                });
                let model = state(&d.w).workspace.layout.header;
                assert_eq!(model.zones[1].len(), 1);
                assert_eq!(model.zones[1][0].item, HeaderItem::Workspaces);
                assert_eq!(model.zones[0], original.zones[0]);
                assert_eq!(model.zones[2], original.zones[2]);
                d.click_name("header-edit-cancel");
                assert_eq!(state(&d.w).workspace.layout.header, original);
            }
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_window_actions_input"]
fn native_header_window_actions_input() {
    let mut d = Driver::new("art.capycanvas.HeaderWindowActions");
    assert!(
        !state(&d.w)
            .workspace
            .layout
            .header
            .entries()
            .any(|e| e.item == HeaderItem::Fullscreen)
    );
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Illustrator.layout(Platform::Gtk),
            ..WorkspaceState::default()
        }),
    });
    pump(500);
    let model = state(&d.w).workspace.layout.header;
    let settings = model
        .entries()
        .find(|e| e.item == HeaderItem::Settings)
        .unwrap()
        .id;
    assert!(!model.entries().any(|e| e.item == HeaderItem::Fullscreen));
    // Portable/saved Web components must not reintroduce a native fullscreen
    // button, reserve a gap, or appear in the overflow menu/editor palette.
    d.w.dispatch(
        HeaderAction::Add {
            zone: HeaderZone::Right,
            before: Some(settings),
            item: HeaderItem::Fullscreen,
        }
        .action(),
    );
    let model = state(&d.w).workspace.layout.header;
    let ids: Vec<_> = [
        HeaderItem::Clock,
        HeaderItem::Battery,
        HeaderItem::Fullscreen,
    ]
    .map(|item| model.entries().find(|e| e.item == item).unwrap().id)
    .into();
    d.w.system_status
        .show_battery(Some(crate::system_status::Battery {
            percent: 73,
            charging: false,
            low: false,
        }));
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(250);
        let geometry = d.w.header.geometry_for_test();
        for id in &ids {
            assert!(!geometry.items.iter().any(|item| item.id == *id));
        }
        assert!(!d.w.system_status.clock.is_visible());
        assert!(!d.w.system_status.battery.is_visible());
        d.edit();
        for id in &ids[..2] {
            assert!(
                d.w.header
                    .geometry_for_test()
                    .items
                    .iter()
                    .any(|item| item.id == *id),
                "status placeholders are editable while windowed"
            );
        }
        assert!(
            !d.w.header
                .geometry_for_test()
                .items
                .iter()
                .any(|item| item.id == ids[2])
        );
        assert!(find_named(d.w.header.root.upcast_ref(), "header-add-full-screen").is_none());
        d.click_name("header-edit-done");
        d.click_name(&format!("header-item-{settings}"));
        assert!(state(&d.w).settings_open && d.w.window.visible_dialog().is_some());
        crate::capture(
            &d.w,
            d.dir
                .join(format!("settings-open-{size:?}.png"))
                .to_str()
                .unwrap(),
        );
        d.key(0xff1b);
        pump(400); // Adwaita emits closed after its closing animation.
        if state(&d.w).settings_open {
            crate::capture(
                &d.w,
                d.dir.join("settings-not-closed.png").to_str().unwrap(),
            );
            eprintln!(
                "Settings focus: {:?}, can close {}",
                gtk::prelude::GtkWindowExt::focus(&d.w.window),
                d.w.preferences.dialog.can_close()
            );
        }
        assert!(!state(&d.w).settings_open);
        for enabled in [true, false] {
            d.key(0xffc8); // Native fullscreen remains available through F11/menu.
            pump(500);
            assert_eq!(d.w.window.is_fullscreen(), enabled);
            assert_eq!(state(&d.w).fullscreen, enabled);
            assert_eq!(d.w.system_status.clock.is_visible(), enabled);
            assert_eq!(d.w.system_status.battery.is_visible(), enabled);
            let geometry = d.w.header.geometry_for_test();
            for id in &ids[..2] {
                assert_eq!(geometry.items.iter().any(|item| item.id == *id), enabled);
            }
            assert!(!geometry.items.iter().any(|item| item.id == ids[2]));
            crate::capture(
                &d.w,
                d.dir
                    .join(format!("status-{size:?}-{enabled}.png"))
                    .to_str()
                    .unwrap(),
            );
        }
    }
    assert_eq!(WorkspacePreset::Illustrator.name(), "Paint");
    assert_eq!(WorkspacePreset::Painter.name(), "Sketch");
    assert_eq!(WorkspacePreset::Photographer.name(), "Photo");
    d.finish();
}

#[path = "selection_tests.rs"]
mod selection_tools;
#[path = "toolbar_component_tests.rs"]
mod toolbar_components;
