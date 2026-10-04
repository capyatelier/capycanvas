use super::*;
use layer_ui::{LayerAction as A, LayerRelationKind as R};

#[test]
#[ignore = "private Wayland display and native menu input"]
fn native_layer_settings_menu() {
    let app = native_test_app("art.capycanvas.LayerSettingsMenu");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    relationship_action(&w, A::New { group: true, clipped: false });
    let group = state(&w).layer_tools.editing_layer.as_ref().unwrap().id;
    let row = find_named(w.layer_panel.root.upcast_ref(), &format!("art-layer-{group}")).unwrap();
    let meta = find_css(&row, "layer-meta").unwrap();
    let attachment = named::<gtk::ToggleButton>(w.layer_panel.root.upcast_ref(), "layer-attachment");
    let item = |popover: &gtk::Popover, label: &str| {
        widgets(popover.upcast_ref()).find(|node| node.is_mapped() && node.type_().name() == "GtkModelButton"
            && node.property::<String>("text") == label).unwrap()
    };
    let open = |input: &mut RemoteInput| {
        let point = screen_point(&find_css(&row, "layer-name").unwrap(), &w.surface, [0.5, 0.5]);
        input.perform(serde_json::json!([{"point":point},{"button":273,"down":true},{"button":273,"down":false}]));
        let popover = w.popovers.borrow().iter().filter_map(|p| p.upgrade())
            .find(|p| p.is_visible() && mapped_label(p.upcast_ref(), "Layer Settings").is_some()).unwrap();
        let settings = item(&popover, "Layer Settings");
        let nested = settings.property::<Option<gtk::PopoverMenu>>("popover").unwrap();
        let point = screen_point(&settings, &w.window, [0.5, 0.5]);
        input.perform(serde_json::json!([{"point":point},{"wait_ms":200}]));
        if !nested.is_mapped() { input.click(point); }
        until(|| nested.is_mapped() && mapped_label(nested.upcast_ref(), "Clip to Layer Below").is_some(), "Layer Settings opens");
        nested.upcast::<gtk::Popover>()
    };
    let icon = |node: &gtk::Widget, name: &str| {
        let image = widgets(node).filter_map(|child| child.downcast::<gtk::Image>().ok())
            .find(|image| crate::icons::name(image).as_deref() == Some(name)).unwrap();
        assert_eq!(image.paintable().unwrap().type_().name(), "GtkSvg");
    };
    let directory = std::path::Path::new("../../artifacts/layer-settings/gtk").join(std::process::id().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let mut input = RemoteInput::new().settle_ms(150);
    input.ready();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        assert!(!meta.is_visible());
        for checked in [false, true] {
            let popover = open(&mut input);
            let pass = item(&popover, "Pass Through");
            let clip = item(&popover, "Clip to Layer Below");
            assert_eq!(pass.property::<bool>("active"), checked);
            assert_eq!(clip.is_sensitive(), !checked);
            icon(&pass, "layer-group-pass-through-symbolic");
            icon(&clip, "layer-clip-symbolic");
            capture_popover(popover.upcast_ref(), directory.join(format!("{theme:?}-pass-{checked}.png")).to_str().unwrap());
            input.click(screen_point(&pass, &w.window, [0.5, 0.5]));
            until(|| state(&w).layers.iter().find(|r| r.id == group).unwrap().pass_through != checked, "Pass Through toggles");
            assert_eq!(meta.is_visible(), !checked);
        }
        for checked in [false, true] {
            let popover = open(&mut input);
            let clip = item(&popover, "Clip to Layer Below");
            assert_eq!(clip.property::<bool>("active"), checked);
            assert!(clip.is_sensitive());
            icon(&clip, "layer-clip-symbolic");
            assert_eq!(attachment.tooltip_text().as_deref(), Some(if checked { "Clipped to Current ink" } else { "Clip to Current ink" }));
            capture_popover(popover.upcast_ref(), directory.join(format!("{theme:?}-clip-{checked}.png")).to_str().unwrap());
            input.click(screen_point(&clip, &w.window, [0.5, 0.5]));
            until(|| state(&w).layer_tools.attachment.checked != checked, "Clipping toggles");
        }
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        pump(100);
        assert!(state(&w).layer_tools.attachment.checked);
        w.dispatch(UiAction::Invoke { command: CommandId::Redo });
        pump(100);
        assert!(!state(&w).layer_tools.attachment.checked);
        for reference in [true, false] {
            let popover = open(&mut input);
            let control = item(&popover, "Use as reference");
            assert_eq!(control.property::<bool>("active"), !reference);
            input.click(screen_point(&control, &w.window, [0.5, 0.5]));
            until(|| state(&w).layers.iter().find(|r| r.id == group).unwrap().reference == reference, "Reference use toggles");
            icon(&row, if reference { "layer-reference-symbolic" } else { "layer-selection-checked-symbolic" });
            save_snapshot(&w, 100, || directory.join(format!("{theme:?}-reference-{reference}.png")));
            if reference {
                relationship_action(&w, A::Select { id: 1, mask: false });
                icon(&row, "layer-reference-symbolic");
                input.click(screen_point(&find_css(&row, "layer-name").unwrap(), &w.surface, [0.5, 0.5]));
                until(|| state(&w).layer_tools.editing_layer.as_ref().unwrap().id == group, "Reference group selected");
                icon(&row, "layer-reference-symbolic");
                relationship_action(&w, A::ToggleSelection { id: 1 });
                icon(&row, "layer-selection-checked-symbolic");
                relationship_action(&w, A::ToggleSelection { id: 1 });
                icon(&row, "layer-reference-symbolic");
            }
        }
    }
    input.finish();
    w.window.destroy();
    pump(100);
}

fn relationship_action(w: &Rc<Workspace>, action: A) {
    w.dispatch(UiAction::Layer { action });
    pump(100);
    assert!(!w.status.is_visible(), "{}", w.status.text());
}

fn relationship_swipe(input: &mut RemoteInput, w: &Workspace, device: &str, id: u64, delta: f32, release: bool) {
    let row = find_named(w.layer_panel.root.upcast_ref(), &format!("art-layer-{id}")).unwrap();
    let start = screen_point(&find_css(&row, "layer-name").unwrap(), &w.surface, [0.25, 0.5]);
    let end = [start[0] + delta, start[1]];
    if delta != 0. { input.perform(serde_json::json!([contact(device,"down",start),contact(device,"move",end)])); }
    if release { input.perform(serde_json::json!([contact(device,"up",end)])); }
}

fn relationship_capture(w: &Rc<Workspace>, input: &mut RemoteInput, directory: &std::path::Path, name: &str, ink: ([f32; 2], [u8; 3])) {
    let mut attempt = 0;
    until(|| {
        attempt += 1;
        let capture = format!("{name}-attempt-{attempt}");
        w.window.queue_draw();
        w.wake();
        pump(30);
        input.perform(serde_json::json!([{"capture":capture}]));
        let path = directory.join(format!("{capture}.png"));
        let texture = gdk::Texture::from_bytes(&glib::Bytes::from_owned(std::fs::read(&path).unwrap())).unwrap();
        let width = texture.width() as usize;
        let height = texture.height() as usize;
        let mut pixels = vec![0;width * height * 4];
        texture.download(&mut pixels,width * 4);
        let painted = pixels.chunks_exact(4).filter(|p| p[2] < 120 && p[1] > 120 && p[0] > 120).count();
        let center = ink.0.map(|value| value.round() as usize);
        let mut neutral = 0;
        for y in center[1].saturating_sub(6)..(center[1]+6).min(height) {
            for x in center[0].saturating_sub(6)..(center[0]+6).min(width) {
                let pixel = &pixels[(y * width + x) * 4..][..4];
                if [pixel[2],pixel[1],pixel[0]].iter().zip(ink.1).all(|(actual,expected)| actual.abs_diff(expected) <= 12) { neutral += 1; }
            }
        }
        if painted > 100_000 && neutral > 0 {
            std::fs::rename(path,directory.join(format!("{name}.png"))).unwrap();
            true
        } else {
            eprintln!("capture {name}: painted={painted}, neutral={neutral}, ink={ink:?}");
            std::fs::rename(path,directory.join(format!("{name}-pending.png"))).unwrap();
            false
        }
    }, "fresh compositor capture presents painted canvas and current neutral FX glyph");
}

#[test]
#[ignore = "private Wayland display, compositor capture and tablet proxy"]
fn native_layer_relationship_review() {
    let app = native_test_app("art.capycanvas.LayerRelationships");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1400);
    let row = |id| find_named(w.layer_panel.root.upcast_ref(), &format!("art-layer-{id}")).unwrap();
    let current = || state(&w).layers.iter().find(|r| r.editing).unwrap().id;
    let view = |id| state(&w).layers.into_iter().find(|r| r.id == id).unwrap();
    let select = |id| relationship_action(&w, A::Select { id, mask: false });
    let name = |id, text: &str| relationship_action(&w, A::Rename { id, name: text.into() });
    let order = || state(&w).layers.iter().map(|r| r.id).collect::<Vec<_>>();
    let thumb = |id| if view(id).selection_layer {
        find_named(w.layer_panel.root.upcast_ref(), &format!("selection-load-{id}")).unwrap()
    } else { find_css(&row(id), "layer-thumbnail").unwrap() };
    let swipe = |id| row(id).parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
    let point = |node: &gtk::Widget, at| screen_point(node, &w.surface, at);
    let revision = || ui_session(&w).engine().document().revision;
    let undo = || {
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        pump(160);
    };
    let import = |label: &str, shadow: bool| {
        let source = layer_core::color::source::rgba8_source([2048, 1536], |x, y| {
            let u = (x as f32 - 1024.) / 660.;
            let v = (y as f32 - 760.) / 530.;
            if u * u + v * v > 1. { [0, 0, 0, 0] }
            else if shadow && x > 1024 { [27, 73, 80, 180] }
            else if shadow { [0, 0, 0, 0] }
            else { [75, 174, 158, 255] }
        });
        ui_session_mut(&w).import_layer_source(label, std::sync::Arc::unwrap_or_clone(source)).unwrap();
        w.refresh(regions::ALL);
        w.wake();
        pump(220);
        current()
    };
    let base = import("Base colors", false);
    let shadows = import("Shadows", true);
    relationship_action(&w, A::Clip { id: shadows, value: true });
    let effect = |label: &str, kind: &str| {
        w.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: kind.into() } });
        pump(160);
        let id = current();
        name(id, label);
        relationship_action(&w, A::AttachEffect { id, owner: shadows });
        id
    };
    let blur = effect("Blur", "gaussian_blur");
    let curves = effect("Curves", "curves");
    select(shadows);
    relationship_action(&w, A::New { group: false, clipped: true });
    let highlights = current();
    name(highlights, "Highlights");
    select(highlights);
    relationship_action(&w, A::New { group: true, clipped: false });
    let isolated = current();
    name(isolated, "Isolated group");
    relationship_action(&w, A::Blend { id: isolated, value: 0 });
    select(highlights);
    relationship_action(&w, A::New { group: true, clipped: false });
    let through = current();
    name(through, "Pass Through group");
    if !view(through).pass_through { relationship_action(&w, A::TogglePassThrough { id: through }); }
    select(shadows);
    w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    w.dispatch(UiAction::Invoke { command: CommandId::SaveSelectionLayer });
    pump(220);
    let saved = state(&w).layers.iter().find(|r| r.selection_layer).unwrap().id;
    name(saved, "Saved selection");
    let before_saved_drop = order();
    assert_eq!(ui_session(&w).layer_drop_preview(saved,shadows,0.,layer_ui::LayerDropSurface::Row), Some(layer_ui::LayerDropHint { target: curves, position: layer_ui::LayerDropPosition::Above }));
    relationship_action(&w, A::Drop { id: saved, target: shadows, fraction: 0., surface: layer_ui::LayerDropSurface::Row });
    let rows = order();
    let saved_at = rows.iter().position(|id| *id == saved).unwrap();
    assert_eq!(&rows[saved_at..=saved_at + 3], &[saved, curves, blur, shadows], "saved selection stays above the complete effect chain");
    undo();
    assert_eq!(order(), before_saved_drop, "selection placement is one undo");
    relationship_action(&w, A::Drop { id: saved, target: shadows, fraction: 0., surface: layer_ui::LayerDropSurface::Row });
    relationship_action(&w, A::Deselect);
    let group = state(&w).workspace.layout.panel_group(Panel::Layers).unwrap();
    w.dispatch(UiAction::SelectPanelTab { group, panel: Panel::Layers });
    pump(180);
    assert!(w.layer_panel.root.is_mapped());
    select(curves);
    let relationship = |id, kind, target| {
        let relation = view(id).relationship.unwrap();
        assert_eq!((relation.kind, relation.target), (kind, target));
    };
    for id in [shadows, highlights] { relationship(id, R::Clip, base); }
    for id in [blur, curves] { relationship(id, R::Effect, shadows); }
    assert!(view(saved).relationship.is_none());
    let load_selection = named::<gtk::Button>(w.layer_panel.root.upcast_ref(), &format!("selection-load-{saved}"));
    assert!(load_selection.has_css_class("flat") && load_selection.has_css_class("layer-icon") && !load_selection.has_css_class("layer-thumbnail"), "Use Selection follows the normal button style");
    assert!(state(&w).layer_tools.connections.iter().any(|c| c.kind == R::Effect && c.from == blur && c.to == shadows));
    let attachment = named::<gtk::ToggleButton>(w.layer_panel.root.upcast_ref(), "layer-attachment");
    for (id, caption) in [(highlights, "Clipped to Base colors"), (curves, "Applied to Shadows")] {
        select(id);
        assert_eq!(attachment.tooltip_text().as_deref(), Some(caption));
        assert!(attachment.is_active());
    }
    attachment.emit_clicked();
    pump(160);
    assert_eq!(attachment.tooltip_text().as_deref(), Some("Apply to Highlights"));
    assert!(!attachment.is_active());
    attachment.emit_clicked();
    pump(160);
    relationship(curves, R::Effect, highlights);
    undo();
    undo();
    relationship(curves, R::Effect, shadows);
    let mut input = RemoteInput::new().settle_ms(180).timeout_secs(15);
    input.ready();
    if std::env::var_os("LAYER_RELATIONSHIP_PEN").is_some() {
        let before = revision();
        relationship_swipe(&mut input, &w, "pen", base, 12., true);
        assert_eq!(revision(), before, "short pen swipe creates no edit");
        relationship_swipe(&mut input, &w, "pen", base, 60., false);
        let controllers = swipe(base).observe_controllers();
        let gesture = (0..controllers.n_items()).find_map(|i| controllers.item(i).and_downcast::<gtk::GestureDrag>()).unwrap();
        gesture.set_state(gtk::EventSequenceState::Denied);
        relationship_swipe(&mut input, &w, "pen", base, 0., true);
        assert_eq!(revision(), before, "cancelled pen swipe creates no edit");
        relationship_swipe(&mut input, &w, "pen", base, 60., true);
        assert!(view(base).alpha_locked);
        undo();
        assert!(!view(base).alpha_locked);
        let before_order = order();
        let before_artwork = artwork_manifest(ui_session(&w).engine().document());
        assert!(ui_session(&w).engine().can_redo());
        let source = row(curves);
        let start = point(&find_css(&source,"layer-name").unwrap(),[0.5,0.5]);
        let end = [start[0],start[1]+30.];
        input.perform(serde_json::json!([contact("pen","down",start),contact("pen","move",end),contact("pen","up",end)]));
        assert_eq!(order(), before_order, "unheld pen body cannot reorder");
        input.perform(serde_json::json!([contact("pen","down",start)]));
        pump(800);
        assert!(w.popovers.borrow().iter().filter_map(|p| p.upgrade()).any(|p| p.is_visible()), "native pen hold arms the row and opens its context menu");
        let controllers = source.observe_controllers();
        let drag = (0..controllers.n_items()).find_map(|i| controllers.item(i).and_downcast::<gtk::DragSource>()).unwrap();
        assert!(drag.emit_by_name::<Option<gdk::ContentProvider>>("prepare", &[&80f64,&18f64]).is_some());
        input.perform(serde_json::json!([contact("pen","up",start)]));
        input.key(65307);
        assert_eq!(order(), before_order);
        assert_eq!(artwork_manifest(ui_session(&w).engine().document()), before_artwork, "pen pickup without drag preserves the artwork");
        assert!(ui_session(&w).engine().can_redo(), "pen pickup cannot consume the undone alpha-lock edit");
        input.finish();
        w.window.destroy();
        pump(100);
        return;
    }
    let captures = std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").unwrap();
    let mut measurements = Vec::new();
    let neutral_ink = || {
        let connections = find_css(w.layer_panel.root.upcast_ref(), "layer-connections").unwrap();
        let parent = connections.parent().unwrap();
        assert!(parent.is_mapped());
        let rgb = |color: gdk::RGBA| [color.red(),color.green(),color.blue()].map(|channel| (channel * 255.).round() as u8);
        let ink = rgb(parent.color());
        assert_eq!(ink,state(&w).palette.text.0,"FX uses the mapped neutral panel foreground");
        let top = point(&thumb(curves),[0.5,1.]);
        let bottom = point(&thumb(blur),[0.5,0.]);
        ([(top[0]+bottom[0])*0.5,(top[1]+bottom[1])*0.5],ink)
    };
    for width in [layer_ui::LAYERS_MIN_WIDTH, 300.] {
        let mut workspace = state(&w).workspace;
        let group = workspace.layout.panel_group(Panel::Layers).unwrap();
        let column = workspace.layout.column_for_group(group).unwrap();
        workspace.layout.bands.iter_mut().find(|b| b.root.id() == column).unwrap().extent = width;
        w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
        pump(350);
        let dimensions = (row(1).width(), row(1).height());
        for theme in [Theme::Light, Theme::Dark] {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            pump(350);
            let connections = find_css(w.layer_panel.root.upcast_ref(), "layer-connections").unwrap();
            assert_eq!(connections.type_().name(), "CapyLayerConnections");
            let color = connections.color();
            let actual = [color.red(), color.green(), color.blue()].map(|channel| (channel * 255.).round() as u8);
            assert_eq!(actual, state(&w).palette.relationship.0, "native connections follow the current theme");
            assert!(w.layer_panel.root.measure(gtk::Orientation::Horizontal, -1).0 <= layer_ui::LAYERS_MIN_WIDTH as i32);
            let badge = find_css(&row(through), "layer-group-pass-through").unwrap();
            assert!(badge.is_visible());
            assert!(!find_css(&row(isolated), "layer-group-pass-through").unwrap().is_visible());
            assert!(view(isolated).description.is_empty());
            assert!(view(through).description.contains("Pass Through"));
            for id in [isolated, through] {
                let subtitle = find_css(&row(id), "layer-meta").unwrap().downcast::<gtk::Label>().unwrap();
                assert_eq!(subtitle.text().as_str(), view(id).description);
            }
            let badge_bounds = badge.compute_bounds(&thumb(through)).unwrap();
            assert!(badge_bounds.x() >= 0. && badge_bounds.y() >= 0.);
            assert!(badge_bounds.x() + badge_bounds.width() <= thumb(through).width() as f32);
            for id in [base, shadows, highlights, blur, curves, isolated, through, saved] {
                let root = row(id);
                assert_eq!((root.width(), root.height()), dimensions, "relationship indicators preserve row allocation");
                let t = thumb(id).compute_bounds(&w.layer_panel.root).unwrap();
                assert_eq!([t.width(),t.height()], [30.,30.]);
                let label = find_css(&root, "layer-name").unwrap().compute_bounds(&w.layer_panel.root).unwrap();
                assert!(label.x() >= t.x() + t.width());
                measurements.push(serde_json::json!({"theme":format!("{theme:?}"),"panel_width":w.layer_panel.root.width(),"id":id,"row":[root.width(),root.height()],"thumbnail":[t.x(),t.y(),t.width(),t.height()],"name_x":label.x(),"relationship_color":actual}));
            }
            let theme = format!("{theme:?}").to_lowercase();
            let capture = format!("relationships-{theme}-{}",width as i32);
            relationship_capture(&w, &mut input, std::path::Path::new(&captures), &capture, neutral_ink());
        }
    }
    std::fs::write(std::path::Path::new(&captures).join("geometry.json"), serde_json::to_vec_pretty(&measurements).unwrap()).unwrap();
    relationship_action(&w, A::Visibility { id: shadows, value: false });
    for id in [blur,curves] {
        assert!(view(id).visible && view(id).visibility_blocked, "hidden owner preserves an effect's own visibility");
        relationship(id,R::Effect,shadows);
        let eye = row(id).first_child().unwrap().downcast::<gtk::Button>().unwrap();
        assert!(eye.opacity() < 1.);
        assert_eq!(crate::icons::name(&eye.child().unwrap().downcast::<gtk::Image>().unwrap()).as_deref(), Some("layer-eye-hidden-symbolic"));
    }
    relationship_capture(&w, &mut input, std::path::Path::new(&captures), "relationships-hidden-owner-dark-300", neutral_ink());
    undo();
    for id in [blur,curves] {
        assert!(view(id).visible && !view(id).visibility_blocked);
        relationship(id,R::Effect,shadows);
    }
    if std::env::var_os("LAYER_RELATIONSHIP_CAPTURE_ONLY").is_some() {
        input.finish();
        w.window.destroy();
        pump(100);
        return;
    }
    relationship_action(&w, A::Collapse { id: through });
    assert!(find_css(&row(through), "layer-group-pass-through").unwrap().is_visible());
    relationship_action(&w, A::Collapse { id: through });
    let before = revision();
    relationship_swipe(&mut input, &w, "touch", isolated, 12., true);
    assert_eq!(revision(), before, "short swipe creates no edit");
    relationship_swipe(&mut input, &w, "touch", isolated, -60., true);
    assert!(swipe(isolated).is_open());
    relationship_swipe(&mut input, &w, "touch", isolated, 60., true);
    assert!(!swipe(isolated).is_open());
    assert_eq!(revision(), before, "closing Delete cannot change group mode");
    relationship_swipe(&mut input, &w, "touch", isolated, 60., false);
    let controllers = swipe(isolated).observe_controllers();
    let gesture = (0..controllers.n_items()).find_map(|i| controllers.item(i).and_downcast::<gtk::GestureDrag>()).unwrap();
    gesture.set_state(gtk::EventSequenceState::Denied);
    relationship_swipe(&mut input, &w, "touch", isolated, 0., true);
    assert_eq!(revision(), before, "capture cancellation creates no mode edit");
    relationship_action(&w, A::Blend { id: isolated, value: 1 });
    let isolated_mode = view(isolated).description;
    relationship_swipe(&mut input, &w, "touch", isolated, 60., true);
    assert!(view(isolated).pass_through);
    assert!(!view(isolated).alpha_locked);
    relationship_swipe(&mut input, &w, "touch", isolated, 60., true);
    assert!(!view(isolated).pass_through);
    assert_eq!(view(isolated).description, isolated_mode);
    undo();
    assert!(view(isolated).pass_through, "one undo restores the previous group mode");
    undo();
    assert!(!view(isolated).pass_through);
    relationship_swipe(&mut input, &w, "touch", base, 60., true);
    assert!(view(base).alpha_locked);
    undo();
    assert!(!view(base).alpha_locked);
    for (device, thumbnail) in [("mouse",true),("touch",false)] {
        select(curves);
        let before_order = order();
        let before_revision = revision();
        let source = row(curves);
        let start = point(&find_css(&source,"layer-name").unwrap(),[0.5,0.5]);
        let target = if thumbnail { point(&thumb(isolated),[0.5,0.5]) } else { point(&row(through),[0.7,0.9]) };
        if device != "mouse" {
            let end = [start[0],start[1]+30.];
            input.perform(serde_json::json!([contact(device,"down",start),contact(device,"move",end),contact(device,"up",end)]));
            assert_eq!(order(), before_order, "unheld {device} row body must not reorder");
            assert_eq!(revision(), before_revision);
        }
        input.perform(serde_json::json!([contact(device,"down",start)]));
        if device != "mouse" {
            pump(800);
            assert!(w.popovers.borrow().iter().filter_map(|p| p.upgrade()).any(|p| p.is_visible()), "touch hold opens the row menu before pickup");
        }
        let pickup = [start[0]-30.,start[1]];
        input.perform(serde_json::json!([contact(device,"move",pickup)]));
        let controllers = source.observe_controllers();
        let drag = (0..controllers.n_items()).find_map(|i| controllers.item(i).and_downcast::<gtk::DragSource>()).unwrap();
        assert!(drag.drag().is_some(), "{device} row pickup starts native DND");
        input.perform(serde_json::json!([contact(device,"move",target),contact(device,"move",[target[0]+1.,target[1]]),contact(device,"up",target)]));
        if thumbnail {
            relationship(curves,R::Effect,isolated);
            assert!(view(isolated).right_swipe.is_none(), "owning an effect requires isolation");
        }
        else { assert_ne!(order(),before_order,"row gap reorders"); }
        undo();
        assert_eq!(order(),before_order,"completed native drop is one undo");
        relationship(curves,R::Effect,shadows);
    }
    let show_layers = || {
        let group = state(&w).workspace.layout.panel_group(Panel::Layers).unwrap();
        w.dispatch(UiAction::SelectPanelTab { group, panel: Panel::Layers });
        pump(180);
        assert!(w.layer_panel.root.is_mapped());
    };
    let mouse_drop = |input: &mut RemoteInput, id, target, at| {
        select(id);
        show_layers();
        super::histogram::scroll_to(&row(id));
        super::histogram::scroll_to(&row(target));
        let source = row(id);
        let start = point(&find_css(&source,"layer-name").unwrap(),[0.5,0.5]);
        let end = point(&row(target),at);
        assert!(source.is_mapped() && view(id).can_drop_below, "boundary source is mapped and draggable");
        input.perform(serde_json::json!([contact("mouse","down",start)]));
        input.perform(serde_json::json!([contact("mouse","move",[start[0]-30.,start[1]])]));
        let controllers = source.observe_controllers();
        let drag = (0..controllers.n_items()).find_map(|i| controllers.item(i).and_downcast::<gtk::DragSource>()).unwrap();
        assert!(drag.drag().is_some(), "boundary pickup starts native DND");
        input.perform(serde_json::json!([contact("mouse","move",end),contact("mouse","move",[end[0]+1.,end[1]]),contact("mouse","up",end)]));
        assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
        assert!(!w.status.is_visible(), "{}", w.status.text());
    };
    relationship_action(&w, A::Clip { id: highlights, value: false });
    select(highlights);
    relationship_action(&w, A::New { group: false, clipped: false });
    let standalone = current();
    name(standalone,"Boundary layer");
    for previously_clipped in [false,true] {
        if previously_clipped { relationship_action(&w,A::Clip { id:standalone,value:true });relationship(standalone,R::Clip,highlights); }
        let before = order();
        mouse_drop(&mut input,standalone,curves,[0.7,0.05]);
        assert!(view(standalone).relationship.is_none(), "above the complete FX/clip chain detaches every previous target");
        let rows = order();
        let at = rows.iter().position(|id| *id == standalone).unwrap();
        assert_eq!(&rows[at..at+5], &[standalone,curves,blur,shadows,base]);
        relationship(shadows,R::Clip,base);
        for id in [curves,blur] { relationship(id,R::Effect,shadows); }
        undo();
        assert_eq!(order(),before);
        if previously_clipped { relationship(standalone,R::Clip,highlights); }
        else { assert!(view(standalone).relationship.is_none()); }
    }
    relationship_action(&w,A::Clip { id:standalone,value:false });
    select(standalone);
    w.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "gaussian_blur".into() } });
    pump(160);
    let inserted = current();
    relationship_action(&w, A::Clip { id: inserted, value: false });
    let before = order();
    mouse_drop(&mut input,inserted,curves,[0.7,0.95]);
    relationship(inserted,R::Effect,shadows);
    let rows = order();
    let at = rows.iter().position(|id| *id == curves).unwrap();
    assert_eq!(&rows[at..at+4], &[curves,inserted,blur,shadows], "an internal FX gap remains joined to its owner");
    undo();
    assert_eq!(order(),before);
    assert!(view(inserted).relationship.is_none());
    select(isolated);
    relationship_action(&w, A::New { group: false, clipped: false });
    let group_base = current();
    relationship_action(&w, A::New { group: false, clipped: true });
    let group_owner = current();
    w.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "gaussian_blur".into() } });
    pump(160);
    let group_effect = current();
    relationship_action(&w, A::AttachEffect { id: group_effect, owner: group_owner });
    let before = order();
    mouse_drop(&mut input,standalone,isolated,[0.7,0.5]);
    assert!(view(standalone).relationship.is_none(), "entering a group starts outside its first FX/clip chain");
    assert_eq!(view(standalone).depth,view(group_base).depth);
    let rows = order();
    let at = rows.iter().position(|id| *id == isolated).unwrap();
    assert_eq!(&rows[at..at+5], &[isolated,standalone,group_effect,group_owner,group_base]);
    relationship(group_owner,R::Clip,group_base);
    relationship(group_effect,R::Effect,group_owner);
    undo();
    assert_eq!(order(),before);
    assert!(view(standalone).relationship.is_none());
    let mut idle_scenes = Vec::new();
    while let Some(id) = order().into_iter().find(|id| *id != 1 && *id != 2) {
        relationship_action(&w,A::Delete { id });
    }
    name(1,"Layer");
    select(1);
    relationship_action(&w,A::New { group:false,clipped:true });
    let motion_owner = current();
    name(motion_owner,"Layer");
    let before_frames = ui_session(&w).engine().metrics().frames;
    w.dispatch(UiAction::Effect { action:EffectAction::Insert { effect:"motion_blur".into() } });
    pump(160);
    let motion = current();
    name(motion,"Motion blur");
    relationship_action(&w,A::AttachEffect { id:motion,owner:motion_owner });
    show_layers();
    assert!(std::env::var_os("CAPY_RECOVERY_DIR").is_some(), "idle reproduction requires private recovery storage");
    let idle_scene = |input:&mut RemoteInput,label:&str| {
        super::new_photo::ready(&w);
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(6200) {
            pump(20);
            assert!(state(&w).host_error.is_none(), "{label}: {:?}",state(&w).host_error);
            assert!(!ui_session(&w).rendering_suspended(), "{label}: canvas stopped: {}",w.status.text());
            assert!(!w.status.is_visible(), "{label}: {}",w.status.text());
        }
        let context = glib::MainContext::default();
        context.block_on(w.recovery().flush()).unwrap();
        let restored = context.block_on(w.recovery().read_snapshot()).unwrap();
        let expected = ui_session(&w).engine().document().artwork.clone();
        let mut restored_artwork = restored.document().artwork.clone();
        for (handle,_,output) in expected.outputs.iter() {
            let saved = restored_artwork.outputs.get_mut(handle).unwrap();
            assert!(saved.context.elapsed.is_finite());
            for (effect,phase) in saved.context.phases.iter() {
                assert!(phase.is_finite() && restored.document().artwork.effects.get(*effect).is_some(), "{label}: captured phases reference current effects");
            }
            saved.context = output.context.clone();
        }
        assert_eq!(restored_artwork,expected, "{label}: recovery preserves exact authored state outside live evaluation context");
        let image = context.block_on(read_canvas_pixels(&w,9821)).unwrap();
        let at = (image.height/2*image.stride+image.width/2*4) as usize;
        let pixel = &image.bytes[at..at+4];
        assert!(pixel.iter().all(|v| *v >= 245), "{label}: empty layers retain rendered Paper: {pixel:?}");
        assert!(ui_session(&w).engine().metrics().frames > before_frames, "{label}: native renderer published the changed scene");
        input.perform(serde_json::json!([{"capture":label}]));
        serde_json::json!({"scene":label,"rows":state(&w).layers,"frames":ui_session(&w).engine().metrics().frames,"paper_pixel":pixel,"recovery":w.recovery().published_path()})
    };
    assert_eq!(order(),[motion,motion_owner,1,2]);
    relationship(motion,R::Effect,motion_owner);
    relationship(motion_owner,R::Clip,1);
    idle_scenes.push(idle_scene(&mut input,"relationships-motion-blur-clipped-owner-idle"));
    relationship_action(&w,A::Clip { id:motion_owner,value:false });
    select(1);
    relationship_action(&w,A::New { group:false,clipped:true });
    let separate_clip = current();
    assert_eq!(order(),[motion,motion_owner,separate_clip,1,2]);
    assert!(view(motion_owner).relationship.is_none());
    relationship(motion,R::Effect,motion_owner);
    relationship(separate_clip,R::Clip,1);
    idle_scenes.push(idle_scene(&mut input,"relationships-motion-blur-separate-clip-idle"));
    undo();
    undo();
    assert_eq!(order(),[motion,motion_owner,1,2]);
    relationship(motion_owner,R::Clip,1);
    relationship_action(&w,A::Delete { id:motion });
    relationship_action(&w,A::Clip { id:motion_owner,value:false });
    assert_eq!(order(),[motion_owner,1,2]);
    assert!(view(motion_owner).relationship.is_none());
    idle_scenes.push(idle_scene(&mut input,"relationships-motion-blur-removed-idle"));
    undo();
    undo();
    assert_eq!(order(),[motion,motion_owner,1,2]);
    relationship(motion,R::Effect,motion_owner);
    relationship(motion_owner,R::Clip,1);
    idle_scenes.push(idle_scene(&mut input,"relationships-motion-blur-undo-idle"));
    for _ in 0..2 {
        w.dispatch(UiAction::Invoke { command:CommandId::Redo });
        pump(160);
    }
    assert_eq!(order(),[motion_owner,1,2]);
    assert!(view(motion_owner).relationship.is_none());
    idle_scenes.push(idle_scene(&mut input,"relationships-motion-blur-redo-idle"));
    super::new_photo::ready(&w);
    assert!(state(&w).host_error.is_none() && !ui_session(&w).rendering_suspended());
    assert!(!w.status.is_visible(), "{}",w.status.text());
    std::fs::write(std::path::Path::new(&captures).join("idle-scenes.json"),serde_json::to_vec_pretty(&idle_scenes).unwrap()).unwrap();
    input.finish();
    w.window.destroy();
    pump(100);
}
