use super::workspace_header::Driver;
use super::*;

fn views(d: &Driver) -> Vec<BubbleView> {
    ui_session(&d.w).bubbles()
}

fn find(d: &Driver, source: BubbleSource) -> Option<BubbleView> {
    views(d).into_iter().find(|v| v.source == source)
}

fn center(d: &Driver, id: u32) -> [f32; 2] {
    d.point(&d.named(&format!("bubble-{id}")))
}

fn stroke(d: &mut Driver, device: &str, path: &[[f32; 2]]) {
    let mut events = vec![contact(device, "down", path[0])];
    events.extend(path[1..].iter().map(|&p| contact(device, "move", p)));
    events.push(contact(device, "up", *path.last().unwrap()));
    d.input.perform(serde_json::Value::Array(events));
    pump(350);
}

fn tap(d: &mut Driver, device: &str, at: [f32; 2]) {
    stroke(d, device, &[at]);
}

fn capture(d: &mut Driver, name: &str) {
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        d.input.perform(serde_json::json!([{ "wait_ms": 300 }, { "capture": name }]));
    }
}

fn tile_of(d: &Driver, control: ToolbarControl) -> String {
    let layout = state(&d.w).workspace.layout;
    layout
        .panels
        .iter()
        .flat_map(|p| p.tiles().to_vec())
        .filter(|t| t.control == control)
        .map(|t| format!("tile-{}", t.id))
        .find(|name| find_named(d.w.window.upcast_ref(), name).is_some_and(|w| w.is_mapped()))
        .unwrap_or_else(|| panic!("no shown tile for {control:?}"))
}

fn header_of(d: &Driver, control: ToolbarControl) -> String {
    let id = state(&d.w).workspace.layout.header.entries().find(|e| e.item == HeaderItem::Tool { control }).unwrap().id;
    format!("header-item-{id}")
}

fn tear(d: &mut Driver, device: &str, widget: &str, to: [f32; 2]) {
    let from = d.point(&d.named(widget));
    stroke(d, device, &[from, [from[0], from[1] + 8.], [from[0], from[1] + 70.], [to[0], to[1] - 30.], to]);
}

fn bead_point(d: &Driver, id: u32, bead: Bead) -> Option<[f32; 2]> {
    let view = find_id(d, id)?;
    let b = view.beads.iter().find(|b| b.placement.bead == bead && b.placement.alpha == 1.)?;
    let c = center(d, id);
    Some([c[0] + b.placement.center[0], c[1] + b.placement.center[1]])
}

fn canvas_at(d: &Driver, id: u32, at: [f32; 2]) -> bool {
    let bubble = d.named(&format!("bubble-{id}"));
    d.w.window.pick(at[0] as f64, at[1] as f64, gtk::PickFlags::DEFAULT).is_none_or(|w| w != bubble && !w.is_ancestor(&bubble))
}

fn menu_heading(d: &Driver, label: &str) -> bool {
    d.w.popovers.borrow().iter().filter_map(|p| p.upgrade()).filter(|p| p.is_visible()).any(|p| {
        widgets(p.upcast_ref()).any(|w| w.downcast_ref::<gtk::Label>().is_some_and(|l| l.text() == label && l.is_mapped()))
    })
}

fn find_id(d: &Driver, id: u32) -> Option<BubbleView> {
    views(d).into_iter().find(|v| v.id == id)
}

fn rim(d: &Driver, view: &BubbleView, degrees: f32) -> [f32; 2] {
    let c = center(d, view.id);
    let (radius, angle) = (view.diameter * 0.5 - RING_RIM * 0.5, degrees.to_radians());
    [c[0] + radius * angle.cos(), c[1] + radius * angle.sin()]
}

fn put_back_all(d: &mut Driver, device: &str, name: &str) {
    for view in views(d) {
        let from = if view.kind == BubbleKind::Color { rim(d, &view, 200.) } else { center(d, view.id) };
        stroke(d, device, &[from, [from[0], from[1] - 20.], [from[0], 10.]]);
    }
    if !views(d).is_empty() {
        capture(d, &format!("{name}-left-out"));
    }
    let left: Vec<_> = views(d).iter().map(|v| {
        let widget = d.named(&format!("bubble-{}", v.id));
        (v.kind, center(d, v.id), widget.opacity(), widget.can_target(), v.open)
    }).collect();
    let popovers = d.w.popovers.borrow().iter().filter_map(|p| p.upgrade()).filter(|p| p.is_visible()).count();
    assert!(left.is_empty(), "{name}: dropping on the title bar puts bubbles back: {left:?}, {popovers} popovers open");
}

fn item_point(d: &Driver, id: u32, item: &ChooserItem) -> [f32; 2] {
    let c = center(d, id);
    [c[0] + item.center[0], c[1] + item.center[1]]
}

fn take(d: &mut Driver, grab: BubbleGrab, at: [f32; 2]) -> u32 {
    let viewport = [d.w.surface.width() as f32, d.w.surface.height() as f32];
    for phase in [ContactPhase::Down, ContactPhase::Up] {
        d.w.dispatch(UiAction::Bubble { action: BubbleAction::Drag { grab, phase, position: at, viewport } });
    }
    pump(400);
    views(d).last().unwrap().id
}

fn chooser(d: &mut Driver, device: &str, name: &str, id: u32) {
    let open = find_id(d, id).unwrap();
    let media: Vec<_> = open.chooser.iter().filter(|i| i.kind == ChooserKind::Medium).cloned().collect();
    assert!(media.len() > 2 && open.rings.is_empty(), "{name}: the third tap rings the media around the dial");
    assert_eq!(media.iter().filter(|m| m.shown).count(), 1, "{name}: the current medium shows its brushes");
    assert!(open.chooser.iter().any(|i| i.kind == ChooserKind::Brush), "{name}: brushes curve beside the medium");
    capture(d, &format!("{name}-chooser"));
    let other = media.iter().find(|m| !m.shown && m.enabled).unwrap().clone();
    tap(d, device, item_point(d, id, &other));
    let switched = find_id(d, id).unwrap();
    assert!(switched.chooser.iter().any(|m| m.shown && m.label == other.label), "{name}: tapping a medium fans its brushes");
    capture(d, &format!("{name}-medium"));
    assert!(switched.chooser.iter().any(|i| i.kind == ChooserKind::Brush), "{name}: a medium fans its brushes, even a single one");
    for tag in widgets(d.named(&format!("bubble-{id}")).upcast_ref()).filter(|w| w.has_css_class("bubble-tag") && w.is_mapped()) {
        let label = tag.downcast::<gtk::Label>().unwrap();
        let (_, natural, _, _) = label.measure(gtk::Orientation::Horizontal, -1);
        assert!(natural as f32 > CHOOSER_TAG[0] || !label.layout().is_ellipsized(), "{name}: the name {} is not clipped", label.text());
    }
    let media: Vec<_> = switched.chooser.iter().filter(|i| i.kind == ChooserKind::Medium).cloned().collect();
    let through = media.iter().find(|m| !m.shown && m.enabled && m.label != other.label).unwrap().clone();
    let from = item_point(d, id, &other);
    let via = item_point(d, id, &through);
    let mut events = vec![contact(device, "down", from)];
    for step in 1..=6 {
        let f = step as f32 / 6.;
        events.push(contact(device, "move", [from[0] + (via[0] - from[0]) * f, from[1] + (via[1] - from[1]) * f]));
    }
    d.input.perform(serde_json::Value::Array(events));
    pump(300);
    let fanned = find_id(d, id).unwrap();
    let target = fanned.chooser.iter().rfind(|i| i.kind == ChooserKind::Brush).cloned();
    let finish = target.as_ref().map_or(via, |t| item_point(d, id, t));
    let mut events = Vec::new();
    for step in 1..=6 {
        let f = step as f32 / 6.;
        events.push(contact(device, "move", [via[0] + (finish[0] - via[0]) * f, via[1] + (finish[1] - via[1]) * f]));
    }
    events.push(contact(device, "up", finish));
    d.input.perform(serde_json::Value::Array(events));
    pump(350);
    if let Some(target) = target {
        assert_eq!(Some(state(&d.w).brush.preset), target.preview, "{name}: sliding through a medium to a brush picks it");
    }
    assert!(find_id(d, id).unwrap().chooser.is_empty(), "{name}: picking closes the ring");
    assert!(!find_id(d, id).unwrap().rings.is_empty(), "{name}: picking opens the selected brush's sliders");
}

fn layers_card(d: &mut Driver, device: &str, name: &str) {
    let (item, spot) = (header_of(d, ToolbarControl::Panel { panel: Panel::Layers }), canvas_spot(d, 0.3, 0.6));
    tap(d, device, d.point(&d.named(&item)));
    pump(500);
    let drawer = d.named("tool-drawer");
    assert!(drawer.is_mapped(), "{name}: the title bar opens the Layers drawer");
    let drawer_size = [drawer.width(), drawer.height()];
    layer_header_insets(d, drawer.upcast_ref());
    capture(d, &format!("{name}-layers-drawer"));
    tap(d, device, canvas_spot(d, 0.6, 0.6));
    pump(500);
    assert!(!drawer.is_mapped(), "{name}: a contact on the canvas closes the drawer");
    let from = d.point(&d.named(&item));
    let path = [from, [from[0], from[1] + 8.], [from[0], from[1] + 70.], [spot[0], spot[1] - 30.], spot];
    let mut events = vec![contact(device, "down", path[0])];
    events.extend(path[1..].iter().map(|&p| contact(device, "move", p)));
    d.input.perform(serde_json::Value::Array(events));
    pump(300);
    let layers = find(d, BubbleSource::Control { control: ToolbarControl::Panel { panel: Panel::Layers } })
        .unwrap_or_else(|| panic!("{name}: Layers comes out of the title bar"));
    assert!(layers.card.is_some() && d.named("bubble-card").is_child_visible(), "{name}: Layers opens while it is dragged out");
    d.input.perform(serde_json::json!([contact(device, "up", spot)]));
    pump(350);
    let layers = find_id(d, layers.id).unwrap();
    let card = layers.card.clone().unwrap_or_else(|| panic!("{name}: Layers comes out open"));
    assert!(!card.full && !card.properties);
    pump(300);
    let body = d.named("bubble-card");
    let bounds = body.compute_bounds(&d.w.window).unwrap();
    let inside = [bounds.x() + 3., bounds.y() + bounds.height() * 0.5];
    tap(d, device, inside);
    assert!(find_id(d, layers.id).unwrap().card.is_some(), "{name}: a tap inside the open card keeps it");
    d.input.perform(serde_json::json!([contact(device, "down", inside), { "wait_ms": 900 }, contact(device, "up", inside)]));
    pump(300);
    assert!(menu_open(d) && menu_heading(d, &layers.label), "{name}: a hold inside the card opens the bubble's menu under its name");
    capture(d, &format!("{name}-layers-menu"));
    d.input.key(0xff1b);
    pump(200);
    let (new_layer, layers_before) = (d.point(&d.named("bubble-new-layer")), state(&d.w).layers.len());
    d.input.perform(serde_json::json!([contact(device, "down", new_layer), { "wait_ms": 900 }, contact(device, "up", new_layer)]));
    pump(300);
    assert!(!menu_open(d), "{name}: a hold on a button is not the bubble's");
    assert_eq!(state(&d.w).layers.len(), layers_before + 1, "{name}: it stays the button's press");
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    pump(200);
    let card = find_id(d, layers.id).unwrap().card.unwrap();
    assert_eq!(card.size[0], Panel::Layers.default_width(d.w.surface.width() as f32), "{name}: the card is the drawer's width");
    assert!((body.height() as f32 - card.size[1]).abs() <= 1., "{name}: and the measured drawer height ({} vs {})", body.height(), card.size[1]);
    capture(d, &format!("{name}-layers-card"));
    drag_layer_card_gap(d, device, layers.id, name);
    let before = center(d, layers.id);
    let bounds = body.compute_bounds(&d.w.window).unwrap();
    let grip = [bounds.x() + bounds.width() * 0.5, bounds.y() + bounds.height() - 20.];
    stroke(d, device, &[grip, [grip[0] - 10., grip[1]], [grip[0] - 70., grip[1] - 30.]]);
    pump(500);
    let moved = center(d, layers.id);
    assert!((moved[0] - before[0]).hypot(moved[1] - before[1]) > 30., "{name}: dragging the open card's empty space moves it");
    assert!(find_id(d, layers.id).unwrap().card.is_some(), "{name}: without folding it");
    let properties = d.named("bubble-layer-properties");
    d.click(&properties);
    pump(500);
    assert!(find_id(d, layers.id).unwrap().card.unwrap().properties, "{name}: Layer properties slide up");
    capture(d, &format!("{name}-layers-properties"));
    d.click(&properties);
    pump(500);
    assert!(!find_id(d, layers.id).unwrap().card.unwrap().properties, "{name}: All layers goes back");
    let count = state(&d.w).layers.len();
    let add = d.named("bubble-new-layer");
    tap(d, device, d.point(&add));
    assert_eq!(state(&d.w).layers.len(), count + 1, "{name}: the footer's icon adds a layer");
    let options = BubbleOptions { full: true, ..find_id(d, layers.id).unwrap().options };
    d.w.dispatch(UiAction::Bubble { action: BubbleAction::SetOptions { id: layers.id, options } });
    pump(500);
    let full = d.named("bubble-card");
    assert!(find_named(full.upcast_ref(), "layer-properties-disclosure").is_some_and(|f| f.is_mapped()), "{name}: Full keeps the drawer's footer");
    assert!(full.has_css_class("dock-panel") && full.has_css_class("content-drawer"), "{name}: the card is styled as the drawer");
    assert!((full.width() - drawer_size[0]).abs() <= 1 && (full.height() - drawer_size[1]).abs() <= 2, "{name}: Full is the drawer's size ({}x{} vs {drawer_size:?})", full.width(), full.height());
    assert!(d.named("bubble-new-layer").parent().is_none_or(|p| !p.is_mapped()), "{name}: instead of the simple one");
    capture(d, &format!("{name}-layers-full"));
    layer_header_insets(d, full.upcast_ref());
    drag_layer_card_gap(d, device, layers.id, name);
    d.w.dispatch(UiAction::Bubble { action: BubbleAction::SetOptions { id: layers.id, options: BubbleOptions { full: false, ..options } } });
    pump(400);
    tap(d, device, canvas_spot(d, 0.85, 0.15));
    assert!(find_id(d, layers.id).unwrap().card.is_none(), "{name}: a contact elsewhere folds the card");
    let card = d.named("bubble-card");
    pump(500);
    assert!(!card.is_child_visible(), "{name}: the card finishes folding away");
    let layers = find_id(d, layers.id).unwrap();
    assert!(layers.layer.is_some() && layers.layers > 0, "{name}: the folded card shows the current layer and the count");
    let count = find_css(&d.named(&format!("bubble-{}", layers.id)), "bubble-count").unwrap();
    let bounds = count.compute_bounds(&count).unwrap();
    assert!(bounds.width() == bounds.height() || layers.layers > 9, "{name}: one digit sits in a circle ({}x{})", bounds.width(), bounds.height());
    capture(d, &format!("{name}-layers-folded"));
    tap(d, device, center(d, layers.id));
    pump(400);
    assert!(find_id(d, layers.id).unwrap().card.is_some(), "{name}: tapping unfolds the card");
    let close = ui_session(&d.w).context_menu(ContextTarget::Bubble { id: layers.id }).unwrap().sections.into_iter().flatten().find(|i| i.label == "Close").unwrap();
    d.w.dispatch(close.action.unwrap());
    pump(500);
    assert!(find_id(d, layers.id).unwrap().card.is_none(), "{name}: Close folds it");
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    pump(200);
}

fn layer_header_insets(d: &Driver, root: &gtk::Widget) {
    let blend = find_named(root, "layer-blend").unwrap().compute_bounds(&d.w.window).unwrap();
    let bounds = root.compute_bounds(&d.w.window).unwrap();
    assert!((blend.y() - bounds.y() - (blend.x() - bounds.x())).abs() <= 1., "Layers header keeps equal top and side insets: {blend:?} in {bounds:?}");
}

fn drag_layer_card_gap(d: &mut Driver, device: &str, id: u32, name: &str) {
    let card = d.named("bubble-card");
    let list = widgets(card.upcast_ref()).find(|w| w.has_css_class("layer-list") && w.is_mapped()).unwrap();
    let bounds = list.compute_bounds(&d.w.window).unwrap();
    let rows_bottom = widgets(card.upcast_ref()).filter(|w| w.has_css_class("layer-row") && w.is_mapped())
        .filter_map(|w| w.compute_bounds(&d.w.window)).map(|b| b.y() + b.height()).fold(bounds.y(), f32::max);
    assert!(bounds.y() + bounds.height() > rows_bottom + 10., "{name}: the list has unused space below its rows");
    let from = [bounds.x() + bounds.width() * 0.5, (rows_bottom + bounds.y() + bounds.height()) * 0.5];
    let before = center(d, id);
    if device == "pen" { d.input.perform(serde_json::json!([{ "point": [3., 3.] }])); }
    stroke(d, device, &[from, [from[0] + 12., from[1]], [from[0] + 60., from[1] + 15.]]);
    assert!((center(d, id)[0] - before[0]).abs() > 30., "{name}: unused list space moves the card");
    d.w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
    pump(400);
    assert!((center(d, id)[0] - before[0]).abs() < 1., "{name}: one layout undo restores the card");
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_layers_bubble_background"]
fn native_layers_bubble_background() {
    layers_bubble_background(&["mouse", "touch"]);
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_layers_bubble_background_pen --tablet"]
fn native_layers_bubble_background_pen() {
    layers_bubble_background(&["pen"]);
}

fn layers_bubble_background(devices: &[&str]) {
    let mut d = Driver::new("art.capycanvas.LayersBubbleBackground");
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default()
        }) });
        pump(300);
        let item = header_of(&d, ToolbarControl::Panel { panel: Panel::Layers });
        let id = item.strip_prefix("header-item-").unwrap().parse().unwrap();
        let from = d.point(&d.named(&item));
        tap(&mut d, "mouse", from);
        pump(500);
        layer_header_insets(&d, &d.named("tool-drawer"));
        let spot = canvas_spot(&d, 0.4, 0.5);
        tap(&mut d, "mouse", spot);
        let id = take(&mut d, BubbleGrab::Header { id }, spot);
        let bubble = find_id(&d, id).unwrap();
        for full in [false, true] {
            d.w.dispatch(UiAction::Bubble { action: BubbleAction::SetOptions { id: bubble.id, options: BubbleOptions { full, ..bubble.options } } });
            pump(500);
            if full { layer_header_insets(&d, &d.named("bubble-card")); }
            for device in devices { drag_layer_card_gap(&mut d, device, bubble.id, &format!("{theme:?}-{device}-{full}")); }
        }
    }
    d.finish();
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_bubble_pickup_before_release"]
fn native_bubble_pickup_before_release() {
    let mut d = Driver::new("art.capycanvas.LiveBubblePickup");
    for theme in [Theme::Light, Theme::Dark] {
        for device in ["mouse", "touch"] {
            for (control, header) in [(ToolbarControl::Command { command: CommandId::DrawingBrush }, true),
                (ToolbarControl::Panel { panel: Panel::Layers }, true), (ToolbarControl::ColorPicker, false)] {
                d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
                d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
                    layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default()
                }) });
                pump(300);
                let widget = if header { header_of(&d, control) } else { tile_of(&d, control) };
                let from = d.point(&d.named(&widget));
                d.input.perform(serde_json::json!([contact(device, "down", from), contact(device, "move", [from[0], from[1] + 4.])]));
                assert!(views(&d).is_empty(), "{theme:?}/{device}/{control:?}: the source keeps its contact before canvas entry");
                let spot = canvas_spot(&d, 0.4, 0.5);
                d.input.perform(serde_json::json!([contact(device, "move", spot)]));
                pump(350);
                let bubble = find(&d, BubbleSource::for_control(control).unwrap()).expect("canvas entry creates the bubble before release");
                let native = d.named(&format!("bubble-{}", bubble.id));
                assert!(native.is_mapped() && native.width() > 0 && native.height() > 0, "{theme:?}/{device}/{control:?}: the held bubble is presented");
                if bubble.card.is_some() { assert!(find_named(&native, "bubble-card").unwrap().is_mapped(), "the Layers card opens during the drag"); }
                capture(&mut d, &format!("live-{theme:?}-{device}-{header}-{control:?}"));
                let before = center(&d, bubble.id);
                let next = [spot[0] + 60., spot[1] + 15.];
                d.input.perform(serde_json::json!([contact(device, "move", next)]));
                pump(350);
                assert_eq!(d.named(&format!("bubble-{}", bubble.id)), native, "motion retains the presented widget");
                assert!(center(&d, bubble.id)[0] > before[0] + 30., "the bubble follows the held contact");
                d.input.perform(serde_json::json!([contact(device, "up", next)]));
                pump(300);
                assert!(find_id(&d, bubble.id).is_some());
                d.w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
                pump(300);
                assert!(views(&d).is_empty(), "pickup and release remain one workspace undo step");
            }
        }
    }
    d.finish();
}

fn menu_open(d: &Driver) -> bool {
    d.w.popovers.borrow().iter().filter_map(|p| p.upgrade()).any(|p| p.is_visible() && p.has_css_class("panel-context-menu"))
}

fn toggle_bubble_sliders(d: &mut Driver, device: &str, id: u32) {
    let before = state(&d.w).workspace.layout.bubbles.show_sliders;
    let at = center(d, id);
    d.input.perform(serde_json::json!([contact(device, "down", at), { "wait_ms": 900 }, contact(device, "up", at)]));
    pump(200);
    let menu = d.w.popovers.borrow().iter().filter_map(|popover| popover.upgrade())
        .find(|popover| popover.is_visible() && popover.has_css_class("panel-context-menu")).unwrap();
    let label = ui_session(&d.w).localization().text(MessageId::WORKSPACE_SHOW_SLIDERS_MENU);
    let item = descendants::<gtk::CheckButton>(menu.upcast_ref()).into_iter().find(|item| item.label().as_deref() == Some(label.as_ref())).unwrap();
    assert_eq!(item.is_active(), before);
    let at = d.point(item.upcast_ref());
    tap(d, device, at);
    assert_eq!(state(&d.w).workspace.layout.bubbles.show_sliders, !before);
    assert_eq!(item.is_active(), !before);
    assert!(menu_open(d), "changing Show Sliders keeps the menu open");
    d.input.key(0xff1b);
    pump(200);
}

fn touches(d: &mut Driver, a: [f32; 2], b: [f32; 2], step: [f32; 2], spread: f32) {
    let touch = |phase: &str, slot: u32, point: [f32; 2]| serde_json::json!({ "touch": phase, "slot": slot, "point": point });
    let mut events = vec![touch("down", 0, a), touch("down", 1, b)];
    for i in 1..=8 {
        let f = i as f32 / 8.;
        events.push(touch("move", 0, [a[0] + step[0] * f - spread * f, a[1] + step[1] * f]));
        events.push(touch("move", 1, [b[0] + step[0] * f + spread * f, b[1] + step[1] * f]));
    }
    events.push(serde_json::json!({ "touch": "up", "slot": 0 }));
    events.push(serde_json::json!({ "touch": "up", "slot": 1 }));
    d.input.perform(serde_json::Value::Array(events));
    pump(600);
}

fn color_ring(d: &mut Driver, device: &str, name: &str) {
    let (item, spot) = (header_of(d, ToolbarControl::Color), canvas_spot(d, 0.68, 0.45));
    tear(d, device, &item, spot);
    let color = find(d, BubbleSource::Control { control: ToolbarControl::Color }).unwrap_or_else(|| panic!("{name}: Color comes out of the title bar"));
    assert!(d.named(&format!("bubble-{}", color.id)).is_mapped());
    if color.beads.is_empty() {
        assert_eq!(color.diameter, COLOR_RING_SIZES[1], "{name}: a new ring is the medium circle");
    }
    let c = center(d, color.id);
    let before = state(&d.w).colors.foreground;
    let hold = [c[0] + 8., c[1] - 6.];
    d.input.perform(serde_json::json!([contact(device, "down", hold), { "wait_ms": 1300 }, contact(device, "up", hold)]));
    pump(300);
    assert!(menu_open(d), "{name}: holding the wheel opens the ring's menu");
    assert_eq!(state(&d.w).colors.foreground, before, "{name}: a hold keeps the color");
    capture(d, &format!("{name}-color-menu"));
    d.input.key(0xff1b);
    pump(200);
    let then = [hold[0] - 30., hold[1] + 24.];
    d.input.perform(serde_json::json!([contact(device, "down", hold), { "wait_ms": 1300 }, contact(device, "move", [hold[0] - 12., hold[1] + 10.]), contact(device, "move", then), { "wait_ms": 100 }]));
    pump(200);
    assert!(!menu_open(d), "{name}: dragging after a hold closes the menu");
    assert_ne!(state(&d.w).colors.foreground, before, "{name}: and goes on picking");
    d.input.perform(serde_json::json!([contact(device, "up", then)]));
    pump(200);
    assert!(!menu_open(d), "{name}: letting go after that keeps the menu closed");
    let options = BubbleOptions { recent: true, palette: true, pair: true, ..color.options };
    d.w.dispatch(UiAction::Bubble { action: BubbleAction::SetOptions { id: color.id, options } });
    pump(500);
    let transparent = bead_point(d, color.id, Bead::Transparent).unwrap();
    tap(d, device, transparent);
    assert_eq!(state(&d.w).colors.slot, ColorSlot::Transparent, "{name}: the transparent bead");
    let foreground = bead_point(d, color.id, Bead::Foreground).unwrap();
    tap(d, device, foreground);
    assert_eq!(state(&d.w).colors.slot, ColorSlot::Foreground, "{name}: the foreground bead");
    let colors = state(&d.w).colors;
    let background = bead_point(d, color.id, Bead::Background).unwrap();
    let away = [background[0] - foreground[0], background[1] - foreground[1]];
    let length = away[0].hypot(away[1]);
    tap(d, device, [background[0] + away[0] / length * 8., background[1] + away[1] / length * 8.]);
    let chosen = state(&d.w).colors;
    assert_eq!(chosen.slot, ColorSlot::Background, "{name}: tapping the background chooses it");
    assert_eq!((chosen.foreground, chosen.background), (colors.foreground, colors.background), "{name}: nothing swaps");
    tap(d, device, foreground);
    let c = center(d, color.id);
    if device == "mouse" && std::env::var("WAYLAND_DISPLAY").is_ok_and(|display| display.ends_with("-tablet")) {
        let widget = d.named(&format!("bubble-{}", color.id));
        d.input.perform(serde_json::json!([contact("pen", "down", [c[0] + 30., c[1]]), contact("pen", "up", [c[0] + 30., c[1]])]));
        pump(200);
        assert_eq!(widget.cursor().and_then(|c| c.name()).as_deref(), Some("none"), "{name}: a pen on the ring shows no pointer");
        d.input.perform(serde_json::json!([{ "pen": "leave" }, contact("mouse", "move", [c[0] + 30., c[1] + 4.])]));
        pump(200);
        assert!(widget.cursor().is_none(), "{name}: the mouse gets its pointer back");
    }
    d.input.perform(serde_json::json!([contact(device, "down", [c[0] - 12., c[1]])]));
    pump(150);
    let first = state(&d.w).colors.foreground;
    d.input.perform(serde_json::json!([contact(device, "move", [c[0] - 4., c[1] + 6.]), contact(device, "move", [c[0] + 16., c[1] + 14.]), contact(device, "up", [c[0] + 16., c[1] + 14.])]));
    pump(350);
    assert_ne!(state(&d.w).colors.foreground, first, "{name}: dragging on the wheel picks colors");
    for mask in [false, true].into_iter().filter(|mask| !*mask || !std::env::var("WAYLAND_DISPLAY").is_ok_and(|display| display.ends_with("-tablet"))) {
        if mask { d.w.dispatch(UiAction::Invoke { command: CommandId::QuickMask }); pump(200); }
        let before = state(&d.w).display_colors().definition();
        d.input.perform(serde_json::json!([contact(device, "down", hold), { "wait_ms": 1300 }, contact(device, "up", hold)]));
        pump(200);
        assert_eq!(state(&d.w).display_colors().definition(), before, "{name}: holding restores the original paint color; mask={mask}");
        d.input.key(0xff1b);
        pump(200);
        if mask { d.w.dispatch(UiAction::Invoke { command: CommandId::QuickMask }); pump(200); }
    }
    d.input.perform(serde_json::json!([contact(device, "down", hold)]));
    pump(100);
    d.w.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Background } });
    let replacement = layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [0.15, 0.7, 0.3, 1.]).unwrap();
    d.w.dispatch(UiAction::Color { action: ColorAction::Definition { color: replacement } });
    d.input.perform(serde_json::json!([{ "wait_ms": 1300 }, contact(device, "up", hold)]));
    pump(200);
    assert_eq!(state(&d.w).colors.definition(), replacement, "{name}: a retired wheel hold cannot restore into another color slot");
    d.input.key(0xff1b);
    d.w.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Foreground } });
    pump(200);
    let picked = state(&d.w).colors.foreground;
    let size = find_id(d, color.id).unwrap().diameter;
    let out: Vec<_> = (0..=12).map(|i| [c[0] - 4. - size * 1.3 * i as f32 / 12., c[1] + 4.]).collect();
    stroke(d, device, &out);
    pump(400);
    let dragged = center(d, color.id);
    assert!(c[0] - dragged[0] > size * 0.25, "{name}: dragging out past two radii moves the ring ({c:?} -> {dragged:?})");
    assert_eq!(state(&d.w).colors.foreground, picked, "{name}: and keeps the color it had");
    let c = dragged;
    let at = |radius: f32| [c[0] + radius * 205f32.to_radians().cos(), c[1] + radius * 205f32.to_radians().sin()];
    assert!(canvas_at(d, color.id, at(size * 0.5 + 4.)), "{name}: the space between the plate and its beads is canvas");
    assert!(canvas_at(d, color.id, [c[0] + size * 0.46, c[1] + size * 0.46]), "{name}: so are the corners around the plate");
    let rim = at(size * 0.5 - 3.);
    stroke(d, device, &[rim, [rim[0] + 10., rim[1] - 4.], [rim[0] + 60., rim[1] - 20.]]);
    pump(400);
    let moved = center(d, color.id);
    assert!((moved[0] - c[0]).hypot(moved[1] - c[1]) > 30., "{name}: the plate's rim moves the ring");
    if device == "touch" {
        let c = moved;
        let held = state(&d.w).colors.foreground;
        touches(d, c, [c[0] + 70., c[1]], [40., 20.], 30.);
        let after = find_id(d, color.id).unwrap();
        assert!(after.diameter > size + 10., "{name}: two fingers starting on the wheel pinch the ring ({size} -> {})", after.diameter);
        let shifted = center(d, color.id);
        assert!((shifted[0] - c[0]).hypot(shifted[1] - c[1]) > 20., "{name}: and move it");
        assert_eq!(state(&d.w).colors.foreground, held, "{name}: without picking a color");
        d.w.dispatch(UiAction::Bubble { action: BubbleAction::SetOptions { id: color.id, options: BubbleOptions { size, ..after.options } } });
        pump(500);
    }
    let first = bead_point(d, color.id, Bead::Swatch { index: 0 }).unwrap();
    let further = bead_point(d, color.id, Bead::Swatch { index: 4 }).unwrap();
    let picked = state(&d.w).colors.foreground;
    stroke(d, device, &[further, [further[0] - 6., further[1] + 1.], first]);
    assert_eq!(state(&d.w).colors.foreground, picked, "{name}: sliding the palette picks nothing");
    let palettes = bead_point(d, color.id, Bead::Palettes).unwrap();
    tap(d, device, palettes);
    assert!(find_id(d, color.id).unwrap().pill.is_some(), "{name}: the palette bead opens its name");
    capture(d, &format!("{name}-color"));
    tap(d, device, canvas_spot(d, 0.15, 0.2));
    assert!(find_id(d, color.id).unwrap().pill.is_none(), "{name}: a contact elsewhere closes the name");
}

fn take_tile(d: &mut Driver, device: &str, control: ToolbarControl, to: [f32; 2]) {
    let from = d.point(&d.named(&tile_of(d, control)));
    stroke(d, device, &[from, [from[0] + 8., from[1]], [from[0] + 90., from[1]], [to[0] - 30., to[1]], to]);
}

fn brush_controls(d: &mut Driver, device: &str, name: &str) {
    take_tile(d, device, ToolbarControl::ColorPicker, canvas_spot(d, 0.3, 0.3));
    let controls = find(d, BubbleSource::Control { control: ToolbarControl::ColorPicker })
        .unwrap_or_else(|| panic!("{name}: dragging the color picker out of its bar takes a bubble"));
    assert_eq!(controls.rings.len(), 2, "{name}: size and strength are open at once");
    let at = center(d, controls.id);
    let size_ring = controls.rings.iter().find(|r| r.setting == "size").unwrap().clone();
    let strength_ring = controls.rings.iter().find(|r| r.setting != "size").unwrap().clone();
    assert!(size_ring.arc.point(0.5)[0] > 0., "{name}: Size is on the right");
    for gap in [[0., -HALO_RINGS[0]], [0., HALO_RINGS[0]], [TOOL_BUBBLE * 0.5 + 5., 0.]] {
        assert!(canvas_at(d, controls.id, [at[0] + gap[0], at[1] + gap[1]]), "{name}: the space around the sliders at {gap:?} is canvas");
    }
    assert!(!canvas_at(d, controls.id, [at[0] + size_ring.arc.icon_point()[0], at[1] + size_ring.arc.icon_point()[1]]), "{name}: a slider's icon end is the slider");
    let ring = size_ring.arc;
    let arc = |f: f32| [at[0] + ring.point(f)[0], at[1] + ring.point(f)[1]];
    d.w.dispatch(ToolbarNumericBinding::BrushSize.action(3.));
    let before = state(&d.w).brush.diameter;
    d.input.perform(serde_json::json!([contact(device, "down", arc(0.3)), contact(device, "move", arc(0.5)), contact(device, "move", arc(0.75))]));
    pump(300);
    assert_ne!(state(&d.w).brush.diameter, before, "{name}: the Size arc sets the brush size");
    let stamp = find_id(d, controls.id).unwrap().stamp.unwrap_or_else(|| panic!("{name}: a dab follows the contact"));
    let widget = d.named(&format!("bubble-{}", controls.id));
    assert_eq!(widget.cursor().and_then(|c| c.name()).as_deref(), Some("none"), "{name}: the dab replaces the pointer while it shows");
    let under = arc(0.75);
    assert!((at[0] + stamp.center[0] - under[0]).hypot(at[1] + stamp.center[1] - under[1]) < 0.5, "{name}: the dab sits under the contact");
    capture(d, &format!("{name}-brush-controls"));
    d.input.perform(serde_json::json!([contact(device, "up", arc(0.75))]));
    pump(300);
    assert!(find_id(d, controls.id).unwrap().stamp.is_none(), "{name}: the dab goes with the contact");
    if device == "mouse" {
        d.input.perform(serde_json::json!([contact("mouse", "move", [at[0] + 2., at[1] + 2.])]));
        pump(100);
        assert!(widget.cursor().is_none(), "{name}: and the mouse gets its pointer back");
    }
    let strength = strength_ring.arc;
    d.w.dispatch(ToolbarNumericBinding::BrushStrength.action(0.95));
    let before = state(&d.w).tool_settings.iter().find(|f| f.id == strength_ring.setting).unwrap().value;
    stroke(d, device, &[[at[0] + strength.point(0.2)[0], at[1] + strength.point(0.2)[1]], [at[0] + strength.point(0.6)[0], at[1] + strength.point(0.6)[1]]]);
    let after = state(&d.w).tool_settings.iter().find(|f| f.id == strength_ring.setting).unwrap().value;
    assert_ne!(after, before, "{name}: the Strength arc sets the strength");
    let tip = [at[0] + size_ring.arc.point(0.9)[0], at[1] + size_ring.arc.point(0.9)[1]];
    tap(d, device, tip);
    pump(400);
    let set = state(&d.w).brush.diameter;
    d.input.perform(serde_json::json!([contact(device, "down", tip), contact(device, "up", tip), contact(device, "down", tip), contact(device, "up", tip)]));
    pump(300);
    let reset = state(&d.w).brush.diameter;
    assert_ne!(reset, set, "{name}: a double click on the Size arc resets it");
    d.w.dispatch(UiAction::ResetToolSetting { id: "size".into() });
    assert_eq!(state(&d.w).brush.diameter, reset, "{name}: to its default");
    tap(d, device, at);
    let picking = find_id(d, controls.id).unwrap();
    assert!(picking.selected, "{name}: the center starts the color picker");
    assert!(picking.rings.len() == 2 && picking.rings.iter().all(|r| !r.enabled), "{name}: its sliders stay, unavailable while picking");
    capture(d, &format!("{name}-brush-controls-picking"));
    tap(d, device, at);
    assert!(!find_id(d, controls.id).unwrap().selected, "{name}: and stops it");
}

fn history(d: &mut Driver, device: &str, name: &str) {
    take_tile(d, device, ToolbarControl::Command { command: CommandId::Redo }, canvas_spot(d, 0.3, 0.75));
    let source = BubbleSource::Control { control: ToolbarControl::Command { command: CommandId::Undo } };
    let bubble = find(d, source).unwrap_or_else(|| panic!("{name}: dragging redo out takes the undo and redo bubble"));
    take_tile(d, device, ToolbarControl::Command { command: CommandId::Undo }, canvas_spot(d, 0.45, 0.8));
    assert_eq!(views(d).iter().filter(|v| v.source == source).count(), 1, "{name}: undo moves the same bubble");
    let from = canvas_spot(d, 0.7, 0.2);
    stroke(d, device, &[from, [from[0] + 60., from[1] + 10.], [from[0] + 120., from[1]]]);
    let enabled = |d: &Driver| find_id(d, bubble.id).unwrap().buttons.iter().map(|b| b.enabled).collect::<Vec<_>>();
    assert_eq!(enabled(d), [true, false], "{name}: a stroke can be undone");
    let c = center(d, bubble.id);
    if device == "mouse" {
        let widget = d.named(&format!("bubble-{}", bubble.id));
        let marked = |class: &str| find_css(&widget, class).is_some_and(|w| w.has_css_class("bubble-half"));
        d.input.perform(serde_json::json!([contact("mouse", "move", [c[0] - 22., c[1]])]));
        pump(100);
        assert!(marked("hovered"), "{name}: the half under the pointer shows hover");
        let half = find_css(&widget, "hovered").unwrap();
        let [width, height] = HISTORY_BUBBLE;
        assert_eq!([half.width(), half.height()], [height as i32; 2], "{name}: the highlight is a full circle as tall as the capsule");
        let bounds = half.compute_bounds(&d.w.window).unwrap();
        assert!((bounds.x() - (c[0] - width * 0.5)).abs() <= 1., "{name}: reaching the capsule's left end ({} vs {})", bounds.x(), c[0] - width * 0.5);
        capture(d, &format!("{name}-history-hover"));
        d.input.perform(serde_json::json!([contact("mouse", "down", [c[0] - 22., c[1]])]));
        pump(100);
        assert!(marked("pressed"), "{name}: and a pressed state while held");
        d.input.perform(serde_json::json!([contact("mouse", "up", [c[0] - 22., c[1]])]));
        pump(300);
        assert!(!marked("pressed"), "{name}: the press clears on release");
    } else {
        tap(d, device, [c[0] - 22., c[1]]);
    }
    assert!(enabled(d)[1], "{name}: the left half undoes");
    tap(d, device, [c[0] + 22., c[1]]);
    assert!(!enabled(d)[1], "{name}: the right half redoes");
    capture(d, &format!("{name}-history"));
}

fn painting_fades(d: &mut Driver, name: &str, id: u32) {
    let settings = gtk::Settings::default().unwrap();
    let animations = settings.is_gtk_enable_animations();
    settings.set_gtk_enable_animations(true);
    let widget = d.named(&format!("bubble-{id}"));
    let faint = |d: &Driver| d.w.glass.borrow().iter().filter(|r| r.shape[1] < 0.5).count();
    let resting = faint(d);
    let at = center(d, id);
    let from = [at[0] - 140., at[1] + 50.];
    let mut events = vec![contact("mouse", "down", from)];
    for step in 1..=20 {
        events.push(contact("mouse", "move", [from[0] + step as f32 * 12., from[1]]));
    }
    d.input.perform(serde_json::Value::Array(events));
    pump(250);
    assert!(widget.opacity() < 0.25, "{name}: a stroke near a bubble fades it");
    assert!(faint(d) > resting, "{name}: and its glass blur fades with it");
    assert!(find_id(d, id).unwrap().open, "{name}: an open halo stays while painting");
    let frames = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let w = d.w.clone();
    let tick = widget.add_tick_callback(glib::clone!(#[strong] frames, move |widget, _| {
        let weakest = |regions: &[layer_render_wgpu::BackdropRegion]| regions.iter().map(|r| r.shape[1]).fold(1f32, f32::min);
        let canvas = w.gpu.borrow_mut().as_mut().map_or(1., |g| weakest(&g.session.renderer_mut().backdrops));
        frames.borrow_mut().push((widget.opacity(), weakest(&w.glass.borrow()), canvas));
        glib::ControlFlow::Continue
    }));
    d.input.perform(serde_json::json!([contact("mouse", "up", [from[0] + 240., from[1]])]));
    pump(600);
    tick.remove();
    assert_eq!(widget.opacity(), 1., "{name}: it comes back after the stroke");
    assert_eq!(faint(d), resting, "{name}: with its full blur");
    let frames = frames.borrow();
    assert!(frames.iter().any(|f| f.0 > 0.21 && f.0 < 0.999), "{name}: and fades back in: {frames:?}");
    assert!(frames.windows(2).all(|f| (f[1].0 - f[0].0).abs() <= 0.12), "{name}: in one smooth ramp: {frames:?}");
    assert!(frames.windows(2).all(|f| f[1].2 == f[1].1 || f[1].2 == f[0].1), "{name}: its blur keeping pace on the canvas: {frames:?}");
    settings.set_gtk_enable_animations(animations);
    d.w.dispatch(UiAction::SetTheme { theme: Some(state(&d.w).theme) });
    pump(20);
    assert!(widget.opacity() == 1. && widget.can_target(), "{name}: later refreshes keep it back");
}

fn canvas_spot(d: &Driver, x: f32, y: f32) -> [f32; 2] {
    let area = d.w.area.compute_bounds(&d.w.window).unwrap();
    [area.x() + area.width() * x, area.y() + area.height() * y]
}

fn zen(d: &mut Driver, name: &str) {
    let panel = state(&d.w).workspace.layout.panels.iter().find(|p| p.tiles().iter().any(|t| t.control == ToolbarControl::BrushSizeSlider)).unwrap().id;
    let group = state(&d.w).workspace.layout.panel_group(panel).unwrap();
    let hidden = |d: &Driver| d.w.groups.borrow().iter().find(|g| g.id == group).unwrap().root.has_css_class("zen-hidden");
    let option = |d: &Driver| {
        ui_session(&d.w).context_menu(ContextTarget::Ribbon { panel }).unwrap().sections.into_iter().flatten().find(|i| i.label == "Show in Zen mode").unwrap()
    };
    let (item, spot) = (header_of(d, ToolbarControl::Color), canvas_spot(d, 0.6, 0.5));
    tear(d, "mouse", &item, spot);
    assert_eq!(option(d).selected, Some(false));
    d.w.dispatch(option(d).action.unwrap());
    d.w.dispatch(UiAction::Invoke { command: CommandId::ZenMode });
    let away = canvas_spot(d, 0.5, 0.5);
    tap(d, "touch", away);
    pump(400);
    assert!(state(&d.w).workspace.zen_mode);
    assert!(!hidden(d), "{name}: a toolbar shown in Zen stays");
    assert!(!d.w.bubbles.layer.has_css_class("zen-hidden"), "{name}: bubbles stay in Zen");
    capture(d, &format!("{name}-zen-kept"));
    d.w.dispatch(option(d).action.unwrap());
    tap(d, "touch", away);
    pump(400);
    assert!(hidden(d), "{name}: other toolbars hide in Zen");
    let height = d.w.window.height() as f32;
    stroke(d, "touch", &[[3., height * 0.5], [20., height * 0.5], [80., height * 0.5]]);
    pump(300);
    assert!(!hidden(d), "{name}: swiping in from the left edge peeks its toolbar");
    capture(d, &format!("{name}-zen-peek"));
    tap(d, "touch", away);
    pump(400);
    assert!(hidden(d), "{name}: a contact elsewhere hides it again");
    for device in ["mouse", "touch"] {
        let ring = find(d, BubbleSource::Control { control: ToolbarControl::Color }).unwrap();
        let from = rim(d, &ring, 200.);
        let to = [from[0] + 60., from[1] + 10.];
        d.input.perform(serde_json::json!([contact(device, "down", from), contact(device, "move", [from[0] + 20., from[1]]), contact(device, "move", to)]));
        pump(250);
        assert!(hidden(d), "{name}-{device}: moving a bubble in Zen keeps the toolbars hidden");
        d.input.perform(serde_json::json!([contact(device, "up", to)]));
        pump(300);
        assert!(hidden(d), "{name}-{device}: and after it lands");
        let c = center(d, ring.id);
        let hold = [c[0] + 8., c[1] - 6.];
        d.input.perform(serde_json::json!([contact(device, "down", hold), { "wait_ms": 1300 }, contact(device, "up", hold)]));
        pump(300);
        assert!(menu_open(d), "{name}-{device}: a hold opens the bubble's menu in Zen");
        assert!(hidden(d), "{name}-{device}: its menu keeps the toolbars hidden");
        d.input.key(0xff1b);
        pump(200);
        tap(d, device, [c[0] - 10., c[1] + 4.]);
        assert!(hidden(d) && state(&d.w).workspace.zen_mode, "{name}-{device}: picking on a bubble stays in Zen");
    }
    d.w.dispatch(UiAction::Invoke { command: CommandId::ZenMode });
    pump(300);
    pinch(d, name);
    put_back_all(d, "mouse", name);
}

fn pinch(d: &mut Driver, name: &str) {
    let ring = find(d, BubbleSource::Control { control: ToolbarControl::Color }).unwrap();
    let c = center(d, ring.id);
    let radius = ring.diameter * 0.5 - RING_RIM * 0.5;
    let rim = |degrees: f32, spread: f32, shift: f32| {
        let a = degrees.to_radians();
        let r = radius + spread;
        [c[0] + r * a.cos() + shift, c[1] + r * a.sin()]
    };
    let touch = |phase: &str, slot: u32, point: [f32; 2]| serde_json::json!({ "touch": phase, "slot": slot, "point": point });
    let mut events = vec![touch("down", 0, rim(200., 0., 0.)), touch("down", 1, rim(340., 0., 0.))];
    for step in 1..=6 {
        let s = step as f32;
        events.push(touch("move", 0, rim(200., s * 4., s * 6.)));
        events.push(touch("move", 1, rim(340., s * 4., s * 6.)));
    }
    events.push(serde_json::json!({ "touch": "up", "slot": 0 }));
    events.push(serde_json::json!({ "touch": "up", "slot": 1 }));
    d.input.perform(serde_json::Value::Array(events));
    pump(600);
    let after = find(d, BubbleSource::Control { control: ToolbarControl::Color }).unwrap();
    assert!(after.diameter > ring.diameter + 10., "{name}: pinching the rim enlarges the ring ({} -> {})", ring.diameter, after.diameter);
    let moved = center(d, ring.id);
    assert!(moved[0] > c[0] + 15., "{name}: two fingers move the ring");
    capture(d, &format!("{name}-pinched"));
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_watercolor_chooser_spacing"]
fn native_watercolor_chooser_spacing() {
    let mut d = Driver::new("art.capycanvas.WatercolorChooserSpacing");
    for theme in [Theme::Light, Theme::Dark] {
        for device in ["mouse", "touch"] {
            for (place, fraction) in [("center", [0.45, 0.5]), ("corner", [0.96, 0.96])] {
                d.w.dispatch(UiAction::RestoreWorkspace {
                    workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }),
                });
                d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
                pump(300);
                let spot = canvas_spot(&d, fraction[0], fraction[1]);
                let id = take(&mut d, BubbleGrab::Variant { variant: ToolVariant::BrushGroup { group: ToolGroup::Watercolor } }, spot);
                let at = center(&d, id);
                tap(&mut d, device, at);
                let open = find_id(&d, id).unwrap();
                assert!(!open.chooser.is_empty());
                assert_eq!(open.chooser.len(), open.dots.len());
                for pill in &open.chooser {
                    let gap = [(pill.center[0].abs() - BRUSH_PILL[0] * 0.5).max(0.), (pill.center[1].abs() - BRUSH_PILL[1] * 0.5).max(0.)];
                    assert!(gap[0].hypot(gap[1]) >= TOOL_BUBBLE * 0.5 + WORKSPACE_SPACING, "{theme:?}/{device}/{fraction:?}: {} overlaps the material face", pill.label);
                }
                let name = format!("{theme:?}-{device}-{place}-chooser").to_lowercase();
                capture(&mut d, &name);
                let before = state(&d.w).brush.preset;
                let at = center(&d, id);
                tap(&mut d, device, [at[0] + open.chooser[0].center[0].signum() * 24., at[1] + 12.]);
                let closed = find_id(&d, id).unwrap();
                assert_eq!(state(&d.w).brush.preset, before, "tapping the material face must not pick an overlapping brush");
                assert!(closed.chooser.is_empty() && closed.rings.is_empty(), "tapping the material face closes its chooser");
            }
        }
    }
    d.finish();
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_bubble_chooser_sliders"]
fn native_bubble_chooser_sliders() {
    let mut d = Driver::new("art.capycanvas.BubbleChooserSliders");
    for theme in [Theme::Light, Theme::Dark] {
        for device in ["mouse", "touch"] {
            for material in [false, true] {
                d.w.dispatch(UiAction::RestoreWorkspace {
                    workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }),
                });
                d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
                d.w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::PointyPencil as u32 });
                pump(300);
                let grab = if material {
                    BubbleGrab::Variant { variant: ToolVariant::BrushGroup { group: ToolGroup::Pastel } }
                } else {
                    let id = state(&d.w).workspace.layout.header.entries().find_map(|entry| {
                        (entry.item == HeaderItem::Tool { control: ToolbarControl::Command { command: CommandId::DrawingBrush } }).then_some(entry.id)
                    }).unwrap();
                    BubbleGrab::Header { id }
                };
                let spot = canvas_spot(&d, 0.45, 0.5);
                let id = take(&mut d, grab, spot);
                d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
                pump(200);
                let at = center(&d, id);
                tap(&mut d, device, at);
                let name = format!("{theme:?}-{device}-{material}").to_lowercase();
                let clicked = find_id(&d, id).unwrap();
                assert!(clicked.selected && !clicked.rings.is_empty() && clicked.chooser.is_empty(), "{name}: clicking selects the tool and opens its sliders");
                capture(&mut d, &format!("{name}-clicked-settings"));
                let at = center(&d, id);
                tap(&mut d, device, at);
                let pick = find_id(&d, id).unwrap().chooser.into_iter().find(|item| item.kind == ChooserKind::Brush && !item.selected).unwrap();
                let at = item_point(&d, id, &pick);
                tap(&mut d, device, at);
                let selected = find_id(&d, id).unwrap();
                assert_eq!(Some(state(&d.w).brush.preset), pick.preview, "{name}: chooser picks its brush");
                assert!(selected.chooser.is_empty() && !selected.rings.is_empty(), "{name}: chooser pick opens its sliders");
                capture(&mut d, &format!("{name}-picked-settings"));
                let ring = &selected.rings[0];
                let value = |d: &Driver| state(&d.w).tool_settings.iter().find(|setting| setting.id == ring.setting).unwrap().value;
                let before = value(&d);
                let at = center(&d, id);
                let arc = |f| { let point = ring.arc.point(f); [at[0] + point[0], at[1] + point[1]] };
                let path = if ring.fill > 0.5 { [0.8, 0.5, 0.2] } else { [0.2, 0.5, 0.8] };
                stroke(&mut d, device, &path.map(arc));
                assert_ne!(value(&d), before, "{name}: the shown slider changes its setting");
                toggle_bubble_sliders(&mut d, device, id);
                assert!(find_id(&d, id).unwrap().rings.is_empty(), "{name}: unchecking Show Sliders hides the halo");
                let spot = canvas_spot(&d, 0.75, 0.5);
                let other = take(&mut d, BubbleGrab::Variant { variant: ToolVariant::BrushPreset { id: layer_core::DefaultBrushPreset::GPen as u32 } }, spot);
                assert!(find_id(&d, other).unwrap().rings.is_empty(), "{name}: another tool shares Show Sliders");
                d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
                let at = center(&d, id);
                tap(&mut d, device, at);
                let clicked = find_id(&d, id).unwrap();
                assert!(clicked.selected && clicked.rings.is_empty(), "{name}: clicking respects Show Sliders");
                let at = center(&d, id);
                tap(&mut d, device, at);
                let pick = find_id(&d, id).unwrap().chooser.into_iter().find(|item| item.kind == ChooserKind::Brush && item.enabled).unwrap();
                let at = item_point(&d, id, &pick);
                tap(&mut d, device, at);
                assert!(find_id(&d, id).unwrap().rings.is_empty(), "{name}: choosing a brush respects Show Sliders");
                d.w.dispatch(UiAction::Bubble { action: BubbleAction::PutBack { id } });
                let spot = canvas_spot(&d, 0.45, 0.5);
                let id = take(&mut d, grab, spot);
                assert!(!state(&d.w).workspace.layout.bubbles.show_sliders && find_id(&d, id).unwrap().rings.is_empty(), "{name}: dragging out remembers Show Sliders");
                capture(&mut d, &format!("{name}-hidden-settings"));
                toggle_bubble_sliders(&mut d, device, other);
                assert!(state(&d.w).workspace.layout.bubbles.show_sliders);
                assert!(views(&d).iter().any(|view| view.selected && !view.rings.is_empty()), "{name}: checking Show Sliders in another tool's menu opens the selected tool's halo");
            }
        }
    }
}

#[test]
#[ignore = "private display: --native-test=native_sketch_eraser_bubble_light"]
fn native_sketch_eraser_bubble_light() {
    sketch_eraser_bubble(Theme::Light);
}

#[test]
#[ignore = "private display: --native-test=native_sketch_eraser_bubble_dark"]
fn native_sketch_eraser_bubble_dark() {
    sketch_eraser_bubble(Theme::Dark);
}

fn sketch_eraser_bubble(theme: Theme) {
    let mut d = Driver::new("art.capycanvas.SketchEraserBubble");
    let control = ToolbarControl::Command { command: CommandId::Eraser };
    let source = BubbleSource::Control { control };
    for device in ["mouse", "touch"] {
        let name = format!("eraser-{theme:?}-{device}").to_lowercase();
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }),
        });
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        d.w.dispatch(UiAction::SelectLayer { id: 1 });
        d.w.dispatch(UiAction::Invoke { command: CommandId::ClearLayer });
        d.w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::GPen as u32 });
        d.w.dispatch(UiAction::SetBrushSize { value: 96. });
        d.w.dispatch(UiAction::SetColor { rgba: [0., 0., 0., 1.] });
        super::new_photo::ready(&d.w);
        let path = [canvas_spot(&d, 0.55, 0.65), canvas_spot(&d, 0.7, 0.65)];
        stroke(&mut d, "mouse", &path);
        super::new_photo::ready(&d.w);
        let opener = header_of(&d, control);
        let spot = canvas_spot(&d, 0.3, 0.4);
        tear(&mut d, device, &opener, spot);
        capture(&mut d, &format!("{name}-created"));
        let bubble = find(&d, source);
        assert!(bubble.is_some(), "{name}: Eraser header tear creates a retained bubble; sources={:?}", views(&d).iter().map(|view| view.source).collect::<Vec<_>>());
        let bubble = bubble.unwrap();
        assert!(bubble.selected && bubble.open && !bubble.rings.is_empty(), "{name}: Eraser opens its settings");
        assert_eq!(state(&d.w).brush.tool, layer_ui::Tool::Eraser);
        let id = bubble.id;
        let at = center(&d, id);
        tap(&mut d, device, at);
        let options = find_id(&d, id).unwrap();
        assert_eq!(options.chooser.len(), 1, "{name}: the Eraser choice stays available");
        let choice = options.chooser[0].clone();
        assert_eq!(choice.kind, ChooserKind::Tool);
        assert_eq!(choice.label.as_str(), "Eraser");
        capture(&mut d, &format!("{name}-options"));
        let choice_at = item_point(&d, id, &choice);
        tap(&mut d, device, choice_at);
        assert!(find_id(&d, id).unwrap().open && !find_id(&d, id).unwrap().rings.is_empty());
        d.w.dispatch(UiAction::Invoke { command: CommandId::DrawingBrush });
        tap(&mut d, device, at);
        assert_eq!(state(&d.w).brush.tool, layer_ui::Tool::Eraser);
        let reopened = find_id(&d, id).unwrap();
        assert!(reopened.selected && reopened.open && !reopened.rings.is_empty(), "{name}: selecting Eraser restores its settings");
        let ring = reopened.rings.iter().find(|ring| ring.enabled).unwrap().clone();
        let value = |d: &Driver| state(&d.w).tool_settings.iter().find(|setting| setting.id == ring.setting).map(|setting| setting.value);
        let before_setting = value(&d);
        let arc = |fraction| { let point = ring.arc.point(fraction); [at[0] + point[0], at[1] + point[1]] };
        stroke(&mut d, device, &[arc(0.2), arc(0.8)]);
        assert_ne!(value(&d), before_setting, "{name}: Eraser's native setting arc changes its value");
        d.w.dispatch(UiAction::ResetToolSetting { id: ring.setting.into() });
        d.w.dispatch(UiAction::SetBrushSize { value: 128. });
        super::new_photo::ready(&d.w);
        let before = super::place_source::snapshot(&d.w);
        let ink = paper_pixels(&d.w);
        stroke(&mut d, "mouse", &path);
        super::new_photo::ready(&d.w);
        pump(700);
        let erased = super::place_source::snapshot(&d.w);
        assert_ne!(erased, before, "{name}: Eraser changes artwork pixels");
        assert!(paper_pixels(&d.w) > ink + 500, "{name}: Eraser visibly reveals the paper");
        capture(&mut d, &format!("{name}-erased"));
        d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        super::new_photo::ready(&d.w);
        assert_eq!(super::place_source::snapshot(&d.w), before, "{name}: Undo restores exact authored pixels");
        d.w.dispatch(UiAction::Invoke { command: CommandId::Redo });
        super::new_photo::ready(&d.w);
        assert_eq!(super::place_source::snapshot(&d.w), erased, "{name}: Redo restores exact erased pixels");
        assert!(state(&d.w).host_error.is_none());
    }
    d.finish();
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_bubble_edge_dismissal; wide and narrow monitors"]
fn native_bubble_edge_dismissal() {
    let mut d = Driver::new("art.capycanvas.BubbleEdgeDismissal");
    let mut workspace = WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() };
    for zone in &mut workspace.layout.header.zones {
        zone.retain(|entry| matches!(entry.item, HeaderItem::Capy | HeaderItem::Tool { control: ToolbarControl::Color | ToolbarControl::Command { command: CommandId::DrawingBrush } }));
    }
    for panel in workspace.layout.panels.iter().map(|panel| panel.id).collect::<Vec<_>>() {
        workspace.layout.set_panel_visible(panel, false).unwrap();
    }
    let mut checked = 0;
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for device in ["mouse", "touch"] {
            for control in [ToolbarControl::Command { command: CommandId::DrawingBrush }, ToolbarControl::Color] {
                for edge in ["left", "right", "top", "bottom"] {
                    d.w.dispatch(UiAction::RestoreWorkspace {
                        workspace: Box::new(workspace.clone()),
                    });
                    pump(300);
                    let spot = canvas_spot(&d, 0.5, 0.5);
                    let source = header_of(&d, control);
                    tear(&mut d, device, &source, spot);
                    let bubble = find(&d, BubbleSource::Control { control }).expect("native source drag takes a bubble");
                    let option = if bubble.kind == BubbleKind::Color { BubbleOption::Recent } else { BubbleOption::Swap };
                    d.w.dispatch(UiAction::Bubble { action: BubbleAction::Option { id: bubble.id, option } });
                    d.w.dispatch(UiAction::Invoke { command: CommandId::ZenMode });
                    pump(300);
                    assert!(state(&d.w).workspace.zen_mode);
                    let before = state(&d.w).workspace.layout.bubbles;
                    let checkpoint = ui_session(&d.w).engine().checkpoint();
                    let from = if bubble.kind == BubbleKind::Color { rim(&d, &bubble, 180.) } else {
                        let c = center(&d, bubble.id);
                        [c[0] + 12., c[1] + 5.]
                    };
                    let viewport = [d.w.surface.width() as f32, d.w.surface.height() as f32];
                    let at = |distance| match edge {
                        "left" => [distance, from[1]],
                        "right" => [viewport[0] - distance, from[1]],
                        "top" => [from[0], distance],
                        _ => [from[0], viewport[1] - distance],
                    };
                    let name = format!("{theme:?}-{device}-{control:?}-{edge}");
                    d.input.perform(serde_json::json!([contact(device, "down", from), contact(device, "move", at(30.)), contact(device, "move", at(12.))]));
                    assert!(find_id(&d, bubble.id).is_some(), "{name}: outside the 8px tolerance keeps the bubble");
                    d.input.perform(serde_json::json!([contact(device, "move", at(4.))]));
                    assert!(find_id(&d, bubble.id).is_none(), "{name}: outward Move puts it back before release");
                    d.input.perform(serde_json::json!([contact(device, "move", spot), contact(device, "move", [spot[0] + 50., spot[1] + 20.]), contact(device, "up", spot)]));
                    pump(200);
                    assert!(find_id(&d, bubble.id).is_none(), "{name}: late release cannot resurrect it");
                    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint, "{name}: captured motion after dismissal cannot paint");
                    let dismissed = state(&d.w).workspace.layout.bubbles;
                    d.w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
                    pump(300);
                    assert_eq!(state(&d.w).workspace.layout.bubbles, before, "{name}: one workspace Undo restores the same bubble, options and saved place");
                    assert!(find_id(&d, bubble.id).is_some(), "{name}: Undo presents the restored bubble");
                    d.w.dispatch(UiAction::Invoke { command: CommandId::RedoWorkspace });
                    pump(200);
                    assert_eq!(state(&d.w).workspace.layout.bubbles, dismissed, "{name}: Redo keeps the remembered source options");
                    assert!(find_id(&d, bubble.id).is_none(), "{name}: one workspace Redo removes it");
                    if device == "mouse" {
                        d.w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
                        pump(300);
                        let restored = find_id(&d, bubble.id).unwrap();
                        let from = if restored.kind == BubbleKind::Color { rim(&d, &restored, 180.) } else { center(&d, bubble.id) };
                        let widget = d.named(&format!("bubble-{}", bubble.id));
                        d.input.perform(serde_json::json!([contact(device, "down", from), contact(device, "move", [from[0] + 35., from[1] + 25.])]));
                        for controller in widget.observe_controllers().iter::<glib::Object>().map(Result::unwrap) {
                            if let Ok(gesture) = controller.downcast::<gtk::GestureDrag>() { gesture.reset(); }
                        }
                        d.input.perform(serde_json::json!([contact(device, "up", from)]));
                        assert!(find_id(&d, bubble.id).is_some(), "{name}: ordinary native gesture cancellation keeps its bubble");
                        assert_eq!(state(&d.w).workspace.layout.bubbles, before, "{name}: ordinary native gesture cancellation restores its saved arrangement");
                    }
                    checked += 1;
                }
            }
        }
    }
    println!("PASS edge dismissal: {checked} native outward contacts at {}x{}", d.w.surface.width(), d.w.surface.height());
    d.finish();
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_tool_bubbles"]
fn native_tool_bubbles() {
    let mut d = Driver::new("art.capycanvas.ToolBubbles");
    let sketch = || UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }),
    };
    d.w.dispatch(sketch());
    pump(300);
    let (item, control) = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .find_map(|e| match e.item {
            HeaderItem::Tool { control: control @ ToolbarControl::Command { command: CommandId::DrawingBrush } } => Some((e.id, control)),
            _ => None,
        })
        .expect("Sketch keeps Drawing in the title bar");
    let dial = BubbleSource::Control { control };
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        d.w.dispatch(sketch());
        pump(300);
        for device in ["mouse", "touch"] {
            let name = format!("{theme:?}-{device}").to_lowercase();
            let start = d.point(&d.named(&format!("header-item-{item}")));
            let spot = canvas_spot(&d, 0.4, 0.5);
            stroke(&mut d, device, &[start, [start[0], start[1] + 8.], [start[0], start[1] + 60.], [spot[0], spot[1] - 40.], spot]);
            let taken = find(&d, dial).unwrap_or_else(|| panic!("{name}: dragging a title-bar tool out takes a bubble"));
            assert_eq!(views(&d).len(), 1, "{name}");
            assert!(taken.selected && taken.open && !taken.rings.is_empty(), "{name}: a tool comes out selected with its settings around it");
            capture(&mut d, &format!("{name}-taken"));

            d.w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::PointyPencil as u32 });
            d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
            pump(200);
            let at = center(&d, taken.id);
            tap(&mut d, device, at);
            assert!(find(&d, dial).unwrap().selected, "{name}: the first tap selects");
            let open = find(&d, dial).unwrap();
            assert!(open.open && !open.rings.is_empty(), "{name}: the first tap shows settings");
            capture(&mut d, &format!("{name}-settings"));
            pump(300);
            let arcs = d.w.glass.borrow().iter().filter(|r| r.shape[0] == 0.).count();
            assert!(arcs >= open.rings.len(), "{name}: every setting's band is glass ({arcs} arcs)");

            let ring = open.rings[0].clone();
            let value = |d: &Driver| state(&d.w).tool_settings.iter().find(|s| s.id == ring.setting).map(|s| s.value);
            let before = value(&d);
            let arc = |f: f32| [at[0] + ring.arc.point(f)[0], at[1] + ring.arc.point(f)[1]];
            let path = if device == "mouse" { [0.1, 0.4, 0.8] } else { [0.9, 0.6, 0.3] };
            stroke(&mut d, device, &path.map(arc));
            assert_ne!(value(&d), before, "{name}: dragging along the ring changes {}", ring.setting);
            assert!(find(&d, dial).unwrap().open, "{name}: the ring stays open while adjusting");

            painting_fades(&mut d, &name, taken.id);
            tap(&mut d, device, at);
            pump(300);
            chooser(&mut d, device, &name, taken.id);

            let brush = BubbleSource::Control { control: ToolbarControl::Brush { id: layer_core::DefaultBrushPreset::Charcoal as u32 } };
            let spot = canvas_spot(&d, 0.62, 0.45);
            let torn = take(&mut d, BubbleGrab::Variant { variant: ToolVariant::BrushPreset { id: layer_core::DefaultBrushPreset::Charcoal as u32 } }, spot);
            let before = center(&d, taken.id);
            let toward = center(&d, torn);
            stroke(&mut d, device, &[toward, [toward[0] - 20., toward[1]], [before[0] + 40., before[1]], [before[0] + 20., before[1]]]);
            pump(800);
            let pushed = center(&d, taken.id);
            assert!((pushed[0] - before[0]).hypot(pushed[1] - before[1]) > 20., "{name}: a dragged bubble pushes its neighbour");
            capture(&mut d, &format!("{name}-pushed"));
            let torn = find_id(&d, torn).unwrap();
            assert_eq!(state(&d.w).brush.preset, layer_core::DefaultBrushPreset::Charcoal as u32, "{name}: a torn brush comes out chosen");
            d.w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::PointyPencil as u32 });
            pump(200);
            if device == "touch" {
                let at = center(&d, taken.id);
                touches(&mut d, at, [at[0] + 20., at[1] + 4.], [-60., 50.], 0.);
                let shifted = center(&d, taken.id);
                assert!((shifted[0] - at[0]).hypot(shifted[1] - at[1]) > 40., "{name}: two fingers on a face move the bubble");
            }
            if device == "touch" {
                let at = center(&d, torn.id);
                d.input.perform(serde_json::json!([contact(device, "down", at), { "wait_ms": 900 }, contact(device, "up", at)]));
                pump(300);
                let menu = d.w.popovers.borrow().iter().filter_map(|p| p.upgrade()).any(|p| p.is_visible() && p.has_css_class("panel-context-menu"));
                assert!(menu, "{name}: holding a bubble opens its menu");
                assert!(!find(&d, brush).unwrap().selected, "{name}: a hold is not a tap");
                capture(&mut d, &format!("{name}-menu"));
                d.input.key(0xff1b);
                pump(200);
                let at = center(&d, torn.id);
                let to = [at[0] + 50., at[1] + 60.];
                d.input.perform(serde_json::json!([contact(device, "down", at), { "wait_ms": 900 }, contact(device, "move", [at[0] + 20., at[1] + 20.]), contact(device, "move", to), contact(device, "up", to)]));
                pump(400);
                let moved = center(&d, torn.id);
                assert!((moved[0] - at[0]).hypot(moved[1] - at[1]) > 40., "{name}: dragging after a hold moves the bubble");
                let open: Vec<_> = d.w.popovers.borrow().iter().filter_map(|p| p.upgrade()).filter(|p| p.is_visible()).map(|p| p.css_classes()).collect();
                assert!(open.is_empty(), "{name}: and closes its menu: {open:?}");
                assert!(!find(&d, brush).unwrap().selected, "{name}: without choosing it");
                let open: Vec<_> = d.w.popovers.borrow().iter().filter_map(|p| p.upgrade()).filter(|p| p.is_visible()).map(|p| p.css_classes()).collect();
                assert!(open.is_empty(), "{name}: Escape closes the bubble menu: {open:?}");
            }

            put_back_all(&mut d, device, &name);

            brush_controls(&mut d, device, &name);
            history(&mut d, device, &name);

            layers_card(&mut d, device, &name);
            color_ring(&mut d, device, &name);
            put_back_all(&mut d, device, &name);
        }
        zen(&mut d, &format!("{theme:?}").to_lowercase());
    }
    d.finish();
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_bubble_pickup_closes_drawers"]
fn native_bubble_pickup_closes_drawers() {
    let mut d = Driver::new("art.capycanvas.BubbleDrawers");
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for device in ["mouse", "touch"] {
            for (index, control) in [ToolbarControl::Color, ToolbarControl::Panel { panel: Panel::Layers },
                ToolbarControl::Command { command: CommandId::DrawingBrush }].into_iter().enumerate() {
                d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
                    layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..Default::default()
                }) });
                pump(300);
                let header = header_of(&d, control);
                let from = d.point(&d.named(&header));
                tap(&mut d, device, from);
                if state(&d.w).customization.drawer.is_none() { tap(&mut d, device, from); }
                pump(300);
                let drawer = d.named("tool-drawer");
                assert!(drawer.is_mapped(), "{theme:?}/{device}/{control:?}: drawer starts open");
                let to = canvas_spot(&d, 0.45, 0.55);
                d.input.perform(serde_json::json!([contact(device, "down", from),
                    contact(device, "move", [from[0], from[1] + 8.]), contact(device, "move", [from[0], from[1] + 80.]),
                    contact(device, "move", to)]));
                pump(400);
                assert!(state(&d.w).customization.drawer.is_none() && !drawer.is_mapped(), "{theme:?}/{device}/{control:?}: pickup closes drawer");
                let bubble = find(&d, BubbleSource::for_control(control).unwrap()).unwrap();
                assert!(ui_session_mut(&d.w).bubble_placements().iter().any(|p| p.id == bubble.id && p.lifted));
                let end = [to[0] + 60., to[1] + 40.];
                d.input.perform(serde_json::json!([contact(device, "move", end), contact(device, "up", end)]));
                pump(350);
                assert!(find_id(&d, bubble.id).is_some(), "the drag survives its drawer closing");
                capture(&mut d, &format!("drawer-pickup-{}-{device}-{index}", format!("{theme:?}").to_lowercase()));
            }
        }
    }
    d.finish();
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_hdr_color_bubble"]
fn native_hdr_color_bubble() {
    let app = native_test_app("art.capycanvas.HdrColorBubble");
    let w = Workspace::with_project(&app, Some((new_drawing_at(640, 480, layer_core::color::SampleDepth::F16), None)));
    w.window.maximize();
    w.window.present();
    super::new_photo::ready(&w);
    let mut d = Driver::start(w, app);
    for theme in [Theme::Light, Theme::Dark] {
        let name = format!("{theme:?}").to_lowercase();
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let mut workspace = WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..Default::default() };
        workspace.layout.set_panel_visible(Panel::Color, true).unwrap();
        workspace.layout.move_panel([1600., 1000.], Panel::Color, DockTarget::Float { position: [220., 150.] }).unwrap();
        d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
        let mut editing = ui_session(&d.w).editing_state();
        editing.color_library.history = (0..12).map(|i| layer_core::color::RgbColor::from_linear(layer_core::color::RgbSpace::Srgb,
            [0.15 + i as f32 * 0.18, 0.75 - i as f32 * 0.04, 0.35 + i as f32 * 0.03, 1.]).unwrap()).collect();
        let change = ui_session_mut(&d.w).restore_editing(editing);
        d.w.changed(change);
        d.w.dispatch(UiAction::Color { action: ColorAction::Library { action: ColorLibraryAction::Import {
            name: "Studio".into(), swatches: (0..40).map(|i| (format!("Color {}", i + 1), layer_core::color::RgbColor::new(
                layer_core::color::RgbSpace::Srgb, [0.2 + (i % 7) as f32 * 0.1, 0.25 + (i % 5) as f32 * 0.12, 0.3 + (i % 3) as f32 * 0.2, 1.]).unwrap())).collect(),
        } } });
        let palette = state(&d.w).color_library.palettes.iter().find(|p| p.name == "Studio").unwrap().id;
        d.w.dispatch(UiAction::Color { action: ColorAction::Library { action: ColorLibraryAction::SelectPalette { id: palette } } });
        d.w.dispatch(UiAction::SetColor { rgba: [0.2, 0.7, 0.45, 1.] });
        pump(500);
        let source = BubbleSource::Control { control: ToolbarControl::Color };
        let spot = canvas_spot(&d, 0.65, 0.48);
        let widget = header_of(&d, ToolbarControl::Color);
        tear(&mut d, "mouse", &widget, spot);
        let id = find(&d, source).unwrap().id;
        for size in COLOR_RING_SIZES {
            for (palette, pair) in [(false, false), (false, true), (true, true)] {
                let options = BubbleOptions { size, pair, palette, recent: palette, ..Default::default() };
                d.w.dispatch(UiAction::Bubble { action: BubbleAction::SetOptions { id, options } });
                pump(500);
                let view = find_id(&d, id).unwrap();
                let layout = view.color.unwrap();
                let bubble = d.named(&format!("bubble-{id}"));
                let wheel = widgets(&bubble).find_map(|w| w.downcast::<crate::tool_panels::ColorWheel>().ok()).unwrap();
                let scale = wheel.intensity().unwrap();
                assert!(scale.is_visible());
                assert!((wheel.drawing_bounds().0 - layout.wheel_size()).abs() < 0.01, "the arc never shrinks the wheel");
                let c = center(&d, id);
                for p in [[0., -size * 0.5 + 2.], [0., layout.bounds().height - size * 0.5 - 2.]] {
                    assert!(!canvas_at(&d, id, [c[0] + p[0], c[1] + p[1]]), "plate rim belongs to bubble");
                }
                if !pair && !palette { assert!(canvas_at(&d, id, [c[0] + size * 0.5, c[1] + size * 0.5]), "outside the shoulder remains canvas"); }
                capture(&mut d, &format!("hdr-bubble-{name}-{size}-palette-{palette}-pair-{pair}"));
            }
        }
        d.w.dispatch(UiAction::Bubble { action: BubbleAction::SetOptions { id, options: BubbleOptions { pair: true, ..Default::default() } } });
        pump(500);
        for device in ["mouse", "touch"] {
            d.w.dispatch(UiAction::Color { action: ColorAction::HdrIntensity { stops: -1. } });
            let locate = |d: &Driver, stops: f32| {
                let c = center(d, id);
                let p = find_id(d, id).unwrap().color.unwrap().intensity.unwrap().point((stops + 2.) / 8.);
                [c[0] + p[0], c[1] + p[1]]
            };
            let base = state(&d.w).colors.picker_base();
            let points = [locate(&d, -1.), locate(&d, 1.), locate(&d, 3.)];
            stroke(&mut d, device, &points);
            assert!((state(&d.w).colors.hdr_intensity() - 3.).abs() < 0.08, "{theme:?}/{device}: intensity gesture");
            assert_eq!(state(&d.w).colors.picker_base(), base);
            let c = center(&d, id);
            stroke(&mut d, device, &[[c[0] + 20., c[1] - 10.], [c[0] - 15., c[1] + 10.]]);
            assert!((state(&d.w).colors.hdr_intensity() - 3.).abs() < 0.08, "wheel preserves HDR intensity");
            let at = locate(&d, 2.);
            d.input.perform(serde_json::json!([{ "wait_ms": 500 }, contact(device, "down", at), contact(device, "up", at),
                { "wait_ms": 50 }, contact(device, "down", at), contact(device, "up", at)]));
            assert_eq!(state(&d.w).colors.hdr_intensity(), 0., "{theme:?}/{device}: double tap resets intensity");
            let view = find_id(&d, id).unwrap();
            let from = rim(&d, &view, 200.);
            stroke(&mut d, device, &[from, [from[0] + 25., from[1]], [from[0] + 50., from[1]]]);
            assert!(center(&d, id)[0] > c[0] + 30., "plate rim drags the complete bubble");
        }
        let background = bead_point(&d, id, Bead::Background).unwrap();
        let background_size = find_id(&d, id).unwrap().beads.iter().find(|b| b.placement.bead == Bead::Background).unwrap().placement.size;
        tap(&mut d, "mouse", [background[0] + background_size * 0.25, background[1] + background_size * 0.15]);
        assert_eq!(state(&d.w).colors.slot, ColorSlot::Background);
        let transparent = bead_point(&d, id, Bead::Transparent).unwrap();
        tap(&mut d, "touch", transparent);
        assert_eq!(state(&d.w).colors.slot, ColorSlot::Transparent);
        d.w.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Foreground } });
        let bubble = d.named(&format!("bubble-{id}"));
        let wheel = widgets(&bubble).find_map(|w| w.downcast::<crate::tool_panels::ColorWheel>().ok()).unwrap();
        let scale = wheel.intensity().unwrap();
        scale.grab_focus();
        d.input.key(0xff53);
        assert!((state(&d.w).colors.hdr_intensity() - 0.1).abs() < 0.02, "keyboard edits the same intensity");
        d.w.dispatch(UiAction::Color { action: ColorAction::HdrIntensity { stops: -1. } });
        let original = state(&d.w).colors;
        let c = center(&d, id);
        let p = find_id(&d, id).unwrap().color.unwrap().intensity.unwrap().point(0.75);
        let at = [c[0] + p[0], c[1] + p[1]];
        d.input.perform(serde_json::json!([{"touch":"down","slot":0,"point":at}, {"touch":"down","slot":1,"point":[c[0],c[1]-100.]},
            {"wait_ms":700}, {"touch":"up","slot":0}, {"touch":"up","slot":1}]));
        assert_eq!(state(&d.w).colors.definition(), original.definition(), "second finger cancels the intensity edit");
        assert_eq!(state(&d.w).colors.hdr_intensity(), -1.);
        pinch(&mut d, &format!("hdr-bubble-{name}"));
        d.w.dispatch(UiAction::Invoke { command: CommandId::QuickMask });
        pump(250);
        assert!(find_id(&d, id).unwrap().color.unwrap().intensity.is_none(), "mask controls are SDR");
        assert!(!scale.is_visible());
        d.w.dispatch(UiAction::Invoke { command: CommandId::QuickMask });
        pump(250);
        assert!(find_id(&d, id).unwrap().color.unwrap().intensity.is_some());
        capture(&mut d, &format!("hdr-bubble-{name}-review"));
    }
    d.finish();
}

fn wheel_point(d: &Driver, wheel: &crate::tool_panels::ColorWheel, offset: [f32; 2]) -> [f32; 2] {
    let (size, [left, top]) = wheel.drawing_bounds();
    let p = wheel.compute_point(&d.w.window, &gtk::graphene::Point::new(left + size * (0.5 + offset[0]), top + size * (0.5 + offset[1]))).unwrap();
    [p.x(), p.y()]
}

fn finger_tap(d: &mut Driver, points: &[[f32; 2]]) {
    let mut events: Vec<_> = points.iter().enumerate().map(|(slot, p)| serde_json::json!({ "touch": "down", "slot": slot, "point": p })).collect();
    events.push(serde_json::json!({ "wait_ms": 60 }));
    events.extend((0..points.len()).map(|slot| serde_json::json!({ "touch": "up", "slot": slot })));
    d.input.perform(serde_json::Value::Array(events));
    pump(300);
}

fn right_click(d: &mut Driver, at: [f32; 2]) {
    d.input.perform(serde_json::json!([{ "point": at, "button": 273, "down": true }, { "button": 273, "down": false }]));
    pump(300);
}

fn menu_item(d: &Driver, text: &str) -> Option<gtk::Widget> {
    menu_open(d).then(|| mapped_label(d.w.window.upcast_ref(), text)).flatten()
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_color_undo"]
fn native_color_undo() {
    let app = native_test_app("art.capycanvas.ColorUndo");
    let w = Workspace::with_project(&app, Some((new_drawing_at(640, 480, layer_core::color::SampleDepth::F16), None)));
    w.window.maximize();
    w.window.present();
    super::new_photo::ready(&w);
    let mut d = Driver::start(w, app);
    let undo = ui_session(&d.w).localization().text(MessageId::WORKSPACE_COLOR_UNDO).to_string();
    let foreground = |d: &Driver| state(&d.w).colors.foreground;
    for theme in [Theme::Light, Theme::Dark] {
        let name = format!("{theme:?}").to_lowercase();
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let mut workspace = WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..Default::default() };
        workspace.layout.set_panel_visible(Panel::Color, true).unwrap();
        workspace.layout.move_panel([1600., 1000.], Panel::Color, DockTarget::Float { position: [220., 150.] }).unwrap();
        d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
        d.w.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Foreground } });
        d.w.dispatch(UiAction::SetColor { rgba: [0.2, 0.7, 0.45, 1.] });
        pump(500);
        let wheel = d.named("color-wheel").downcast::<crate::tool_panels::ColorWheel>().unwrap();
        let start = foreground(&d);
        let drag = [wheel_point(&d, &wheel, [0.1, -0.1]), wheel_point(&d, &wheel, [0.15, 0.]), wheel_point(&d, &wheel, [0.05, 0.12])];
        stroke(&mut d, "mouse", &drag);
        let dragged = foreground(&d);
        assert_ne!(dragged, start, "{name}: dragging on the panel wheel picks");
        let ring = wheel_point(&d, &wheel, [0.47, 0.]);
        right_click(&mut d, ring);
        let item = menu_item(&d, &undo).unwrap_or_else(|| panic!("{name}: right-clicking the hue ring offers {undo}"));
        capture(&mut d, &format!("color-undo-{name}-panel-menu"));
        let at = d.point(&item);
        d.input.click(at);
        pump(300);
        assert_eq!(foreground(&d), start, "{name}: one undo restores the color from before the whole drag");

        let (width, height) = (wheel.width() as f32, wheel.height() as f32);
        let outside = (1..20).flat_map(|i| (1..20).map(move |j| [width * i as f32 / 20., height * j as f32 / 20.]))
            .find(|&[x, y]| !wheel.contains(x as f64, y as f64) && wheel.pick(x as f64, y as f64, gtk::PickFlags::DEFAULT).is_none())
            .expect("the wheel leaves room outside its ring");
        let outside = wheel.compute_point(&d.w.window, &gtk::graphene::Point::new(outside[0], outside[1])).unwrap();
        right_click(&mut d, [outside.x(), outside.y()]);
        assert!(menu_open(&d) && menu_item(&d, &undo).is_none(), "{name}: outside the ring the panel keeps its own menu");
        d.input.key(0xff1b);
        pump(200);

        let fingers = [wheel_point(&d, &wheel, [-0.12, 0.]), wheel_point(&d, &wheel, [0.12, 0.]), wheel_point(&d, &wheel, [0., 0.47])];
        finger_tap(&mut d, &fingers);
        assert_eq!(foreground(&d), dragged, "{name}: a three-finger tap on the panel wheel redoes");
        finger_tap(&mut d, &fingers[..2]);
        assert_eq!(foreground(&d), start, "{name}: a two-finger tap on the panel wheel undoes");
        stroke(&mut d, "mouse", &drag);
        let field = wheel_point(&d, &wheel, [-0.2, 0.2]);
        finger_tap(&mut d, &[field, ring]);
        assert_eq!(foreground(&d), start, "{name}: the second finger retires the first finger's pick and the tap undoes");

        let scale = wheel.intensity().unwrap();
        assert!(scale.is_visible(), "{name}: HDR drawings show the intensity arc");
        let arc = [0.3, 0.4, 0.5, 0.6, 0.7, 0.8].map(|t| {
            let p = scale.geometry().unwrap().point(t);
            let p = scale.compute_point(&d.w.window, &gtk::graphene::Point::new(p[0], p[1])).unwrap();
            [p.x(), p.y()]
        });
        let stops = state(&d.w).colors.hdr_intensity();
        stroke(&mut d, "mouse", &arc[1..]);
        assert!(state(&d.w).colors.hdr_intensity() > stops + 1., "{name}: the arc raises the intensity");
        finger_tap(&mut d, &[arc[0], arc[4]]);
        assert_eq!(state(&d.w).colors.hdr_intensity(), stops, "{name}: a two-finger tap on the arc undoes the whole arc drag");
        right_click(&mut d, arc[2]);
        assert!(menu_item(&d, &undo).is_some(), "{name}: the arc offers color undo too");
        d.input.key(0xff1b);
        pump(200);

        let source = BubbleSource::Control { control: ToolbarControl::Color };
        let widget = header_of(&d, ToolbarControl::Color);
        let spot = canvas_spot(&d, 0.65, 0.48);
        tear(&mut d, "mouse", &widget, spot);
        let id = find(&d, source).unwrap().id;
        pump(500);
        let c = center(&d, id);
        let before = foreground(&d);
        stroke(&mut d, "touch", &[[c[0] + 20., c[1] - 10.], [c[0] - 15., c[1] + 10.]]);
        let picked = foreground(&d);
        assert_ne!(picked, before, "{name}: the ring picks");
        finger_tap(&mut d, &[[c[0] - 25., c[1]], [c[0] + 25., c[1]]]);
        assert_eq!(foreground(&d), before, "{name}: a two-finger tap on the ring undoes");
        assert_eq!(center(&d, id), c, "{name}: and leaves the ring where it is");
        finger_tap(&mut d, &[[c[0] - 25., c[1]], [c[0] + 25., c[1]], [c[0], c[1] + 30.]]);
        assert_eq!(foreground(&d), picked, "{name}: a three-finger tap on the ring redoes");
        touches(&mut d, c, [c[0] + 70., c[1]], [60., 20.], 0.);
        assert!((center(&d, id)[0] - c[0]).abs() > 30., "{name}: two moving fingers still move the ring");
        assert_eq!(foreground(&d), picked, "{name}: without undoing");
        let moved = center(&d, id);
        right_click(&mut d, moved);
        let item = menu_item(&d, &undo).unwrap_or_else(|| panic!("{name}: the ring's menu offers {undo}"));
        capture(&mut d, &format!("color-undo-{name}-bubble-menu"));
        let at = d.point(&item);
        d.input.click(at);
        pump(300);
        assert_eq!(foreground(&d), before, "{name}: the ring's menu undoes");
    }
    d.finish();
}

#[test]
#[ignore = "private display: LAYER_NATIVE_EVENT_MS=4 workspace-motion.sh gtk --native-test=native_hdr_color_bubble_motion"]
fn native_hdr_color_bubble_motion() {
    let app = native_test_app("art.capycanvas.HdrBubbleMotion");
    let w = Workspace::with_project(&app, Some((new_drawing_at(640, 480, layer_core::color::SampleDepth::F16), None)));
    w.window.maximize();
    w.window.present();
    super::new_photo::ready(&w);
    let mut d = Driver::start(w, app);
    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..Default::default() }) });
    pump(400);
    let header = header_of(&d, ToolbarControl::Color);
    let spot = canvas_spot(&d, 0.55, 0.5);
    tear(&mut d, "mouse", &header, spot);
    let id = find(&d, BubbleSource::Control { control: ToolbarControl::Color }).unwrap().id;
    d.w.dispatch(UiAction::Bubble { action: BubbleAction::SetOptions { id, options: BubbleOptions { pair: true, recent: true, palette: true, ..Default::default() } } });
    pump(500);
    let mut reports = Vec::new();
    for device in ["mouse", "touch"] {
        for mode in ["drag", "intensity"] {
            for run in 0..4 {
                let view = find_id(&d, id).unwrap();
                let origin = center(&d, id);
                let arc = view.color.unwrap().intensity.unwrap();
                let point = |i: usize| {
                    let t = i as f32 * 0.004;
                    let phase = if mode == "intensity" { 1. - 2. * ((t + 0.5).rem_euclid(2.) - 1.).abs() }
                        else { (t * std::f32::consts::TAU * 0.5).sin() };
                    let p = if mode == "intensity" { arc.point(0.5 + phase * 0.3) }
                        else { [phase * 140., -view.diameter * 0.5 + 3. + (t * std::f32::consts::TAU).sin() * 24.] };
                    [origin[0] + p[0], origin[1] + p[1]]
                };
                d.input.perform(serde_json::json!([contact(device, "down", point(0))]));
                let clock = d.w.surface.frame_clock().unwrap();
                let timings = Rc::new(RefCell::new(Vec::new()));
                let paints = Rc::new(RefCell::new(Vec::new()));
                let paint_start = Rc::new(Cell::new(std::time::Instant::now()));
                let before = clock.connect_before_paint(glib::clone!(#[strong] paint_start, move |_| paint_start.set(std::time::Instant::now())));
                let after = clock.connect_after_paint(glib::clone!(#[strong] timings, #[strong] paints, #[strong] paint_start, move |clock| {
                    if let Some(t) = clock.current_timings() { timings.borrow_mut().push(t); }
                    paints.borrow_mut().push(paint_start.get().elapsed().as_secs_f64() * 1000.);
                }));
                let stats = ui_session(&d.w).engine().backend().stats.clone();
                *stats.lock().unwrap() = Default::default();
                let refreshes = d.w.publication.refreshes.get();
                let count = if run == 0 { 250 } else { 1500 };
                d.input.perform(serde_json::Value::Array((1..=count).map(|i| contact(device, "move", point(i))).collect()));
                let end = clock.frame_time();
                let refreshed = d.w.publication.refreshes.get() - refreshes;
                d.input.perform(serde_json::json!([contact(device, "up", point(count))]));
                pump(250);
                clock.disconnect(before);
                clock.disconnect(after);
                if run == 0 { continue; }
                let mut presented: Vec<_> = timings.borrow().iter().filter(|t| t.is_complete() && t.presentation_time() > 0 && t.frame_time() <= end).map(|t| t.presentation_time()).collect();
                presented.sort_unstable(); presented.dedup();
                assert!(presented.len() > 100, "{device}/{mode} produces moving frames");
                let mut gaps: Vec<_> = presented.windows(2).map(|p| (p[1] - p[0]) as f64 / 1000.).collect();
                gaps.sort_by(f64::total_cmp);
                let percentile = |mut values: Vec<f64>| { values.sort_by(f64::total_cmp); values.get(values.len() * 95 / 100).copied() };
                let canvas = {
                    let stats = stats.lock().unwrap();
                    serde_json::json!({"backdrop_frames":stats.backdrop_frames,"frames":stats.cpu.len(),
                        "gpu_p95_ms":percentile(stats.gpu.iter().map(|v|v[1]).collect()),
                        "owner_frame_p95_ms":percentile(stats.frame_handler_cpu.clone()),
                        "owner_input_p95_ms":percentile(stats.input_handler_cpu.clone())})
                };
                let report = serde_json::json!({"device":device,"mode":mode,"run":run,"frames":presented.len(),
                    "presented_hz":(presented.len()-1) as f64 * 1_000_000. / (presented.last().unwrap()-presented[0]) as f64,
                    "gap_p99_ms":gaps[gaps.len()*99/100],"maximum_gap_ms":gaps.last(),"refreshes":refreshed,
                    "paint_p95_ms":percentile(paints.borrow().clone()),"canvas":canvas});
                eprintln!("{report}"); reports.push(report);
            }
        }
    }
    if let Some(dir) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
        std::fs::write(std::path::Path::new(&dir).join("hdr-color-bubble-motion.json"), serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    }
    d.finish();
}

#[test]
#[ignore = "private display: LAYER_NATIVE_EVENT_MS=4 workspace-motion.sh gtk --native-test=native_tool_bubble_motion"]
fn native_tool_bubble_motion() {
    let mut d = Driver::new("art.capycanvas.ToolBubbleMotion");
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }),
    });
    pump(400);
    let viewport = [d.w.surface.width() as f32, d.w.surface.height() as f32];
    let grabs = [
        BubbleGrab::Variant { variant: ToolVariant::BrushPreset { id: layer_core::DefaultBrushPreset::PointyPencil as u32 } },
        BubbleGrab::Variant { variant: ToolVariant::BrushGroup { group: ToolGroup::Oil } },
        BubbleGrab::Variant { variant: ToolVariant::BrushPreset { id: layer_core::DefaultBrushPreset::Charcoal as u32 } },
        BubbleGrab::Variant { variant: ToolVariant::BrushGroup { group: ToolGroup::Watercolor } },
        BubbleGrab::Variant { variant: ToolVariant::BrushPreset { id: layer_core::DefaultBrushPreset::Marker as u32 } },
    ];
    for (i, grab) in grabs.into_iter().enumerate() {
        let at = canvas_spot(&d, 0.3 + i as f32 * 0.1, 0.5);
        for phase in [ContactPhase::Down, ContactPhase::Up] {
            d.w.dispatch(UiAction::Bubble { action: BubbleAction::Drag { grab, phase, position: at, viewport } });
        }
    }
    pump(600);
    let mover = views(&d)[0].id;
    let mut reports = Vec::new();
    for device in ["mouse", "touch"] {
        let origin = center(&d, mover);
        d.input.perform(serde_json::json!([contact(device, "down", origin), contact(device, "move", [origin[0] + 6., origin[1]]), contact(device, "move", [origin[0] + 12., origin[1]])]));
        pump(100);
        let refreshes = d.w.publication.refreshes.get();
        let clock = d.w.surface.frame_clock().unwrap();
        let timings = Rc::new(RefCell::new(Vec::new()));
        let after = clock.connect_after_paint(glib::clone!(#[strong] timings, move |clock| {
            if let Some(t) = clock.current_timings() {
                timings.borrow_mut().push(t);
            }
        }));
        let stats = ui_session(&d.w).engine().backend().stats.clone();
        *stats.lock().unwrap() = Default::default();
        let events: Vec<_> = (0..550)
            .map(|i| {
                let t = i as f32 * 0.004;
                let swing = |p: f32| (p * std::f32::consts::TAU).sin();
                contact(device, "move", [origin[0] + 12. + swing(t * 0.5) * 420., origin[1] + swing(t * 1.5) * 60.])
            })
            .collect();
        d.input.perform(serde_json::Value::Array(events));
        let steady = d.w.publication.refreshes.get();
        let steady_end = clock.frame_time();
        d.input.perform(serde_json::json!([contact(device, "up", [origin[0] + 12., origin[1]])]));
        let released = std::time::Instant::now();
        while ui_session_mut(&d.w).step_bubbles(clock.frame_time().max(0) as u64 * 1000) && released.elapsed().as_secs() < 3 {
            pump(16);
        }
        let settle_ms = released.elapsed().as_secs_f64() * 1000.;
        clock.disconnect(after);
        assert_eq!(steady, refreshes, "{device}: steady bubble motion retains every UI model");
        let mut presented: Vec<_> = timings
            .borrow()
            .iter()
            .filter(|t| t.is_complete() && t.presentation_time() > 0 && t.frame_time() <= steady_end)
            .map(|t| t.presentation_time())
            .collect();
        presented.sort_unstable();
        presented.dedup();
        let mut gaps: Vec<_> = presented.windows(2).map(|p| (p[1] - p[0]) as f64 / 1000.).collect();
        gaps.sort_by(f64::total_cmp);
        let hz = (presented.len().max(2) - 1) as f64 * 1_000_000. / (presented.last().unwrap() - presented[0]).max(1) as f64;
        let canvas = {
            let stats = stats.lock().unwrap();
            let mut gpu: Vec<f64> = stats.gpu.iter().map(|v| v[1]).collect();
            gpu.sort_by(f64::total_cmp);
            serde_json::json!({ "backdrop_frames": stats.backdrop_frames, "frames": stats.cpu.len(), "gpu_ms": gpu.get(gpu.len() / 2), "gpu_p95_ms": gpu.get(gpu.len() * 95 / 100) })
        };
        let report = serde_json::json!({
            "device": device, "presented_frames": presented.len(), "presentation_hz": hz,
            "presentation_gap_p50_ms": gaps.get(gaps.len() / 2), "presentation_gap_p99_ms": gaps.get(gaps.len() * 99 / 100),
            "gaps_over_12_ms": gaps.iter().filter(|g| **g > 12.).count(), "longest_gap_ms": gaps.last(),
            "refresh_intervals_us": timings.borrow().iter().map(|t| t.refresh_interval()).collect::<std::collections::BTreeSet<_>>(),
            "settle_ms": settle_ms, "canvas": canvas,
        });
        eprintln!("{report}");
        reports.push(report);
        let moved = views(&d).iter().filter(|v| v.id != mover).count();
        assert_eq!(moved, 4);
    }
    if let Some(dir) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
        std::fs::write(std::path::Path::new(&dir).join("tool-bubble-motion.json"), serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    }
    d.finish();
}

fn dark_outside_rings(directory: &std::path::Path) -> Vec<(usize, usize)> {
    let frames: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(directory.join("frames.json")).unwrap()).unwrap();
    let mut found = Vec::new();
    for (index, frame) in frames.iter().enumerate() {
        let [width, height, stride] = ["width", "height", "stride"].map(|k| frame[k].as_u64().unwrap() as usize);
        let bytes = std::fs::read(directory.join(format!("{index:04}.rgb"))).unwrap();
        let px = |x: usize, y: usize| &bytes[y * stride + x * 3..][..3];
        let [x0, y0, x1, y1] = [width / 5, height / 6, width * 4 / 5, height * 9 / 10];
        let mut ring = Vec::new();
        for y in (y0..y1).step_by(2) {
            for x in (x0..x1).step_by(2) {
                let p = px(x, y);
                if p.iter().max().unwrap() - p.iter().min().unwrap() > 120 {
                    ring.push([x as f32, y as f32]);
                }
            }
        }
        if ring.len() < 20 {
            continue;
        }
        let center = ring.iter().fold([0.; 2], |a, p| [a[0] + p[0], a[1] + p[1]]).map(|v| v / ring.len() as f32);
        let reach = ring.iter().map(|p| (p[0] - center[0]).hypot(p[1] - center[1])).fold(0., f32::max) + 12.;
        let dark = (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| *px(x, y).iter().max().unwrap() < 24 && (x as f32 - center[0]).hypot(y as f32 - center[1]) > reach)
            .count();
        if dark > 12 {
            found.push((index, dark));
        }
    }
    found
}

#[test]
#[ignore = "private display: LAYER_NATIVE_CAPTURE_DIR=... workspace-motion.sh gtk --native-test=native_tool_bubble_frames"]
fn native_tool_bubble_frames() {
    let mut d = Driver::new("art.capycanvas.ToolBubbleFrames");
    let captures = std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").map(std::path::PathBuf::from);
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }),
        });
        pump(500);
        let name = format!("{theme:?}").to_lowercase();
        let from = d.point(&d.named(&header_of(&d, ToolbarControl::Color)));
        let spot = canvas_spot(&d, 0.5, 0.55);
        let mut events = vec![serde_json::json!({ "record_start": format!("{name}-tear") })];
        events.extend([from, [from[0], from[1] + 8.], [from[0], from[1] + 70.], [spot[0], spot[1] - 30.], spot].iter().enumerate().map(|(i, p)| contact("mouse", if i == 0 { "down" } else { "move" }, *p)));
        events.extend([contact("mouse", "up", spot), serde_json::json!({ "wait_ms": 500 }), serde_json::json!({ "record_stop": true })]);
        d.input.perform(serde_json::Value::Array(events));
        pump(300);
        let ring = find(&d, BubbleSource::Control { control: ToolbarControl::Color }).unwrap();
        let c = center(&d, ring.id);
        let radius = ring.diameter * 0.5 - RING_RIM * 0.5;
        let touch = |phase: &str, slot: u32, point: [f32; 2]| serde_json::json!({ "touch": phase, "slot": slot, "point": point });
        let mut events = vec![serde_json::json!({ "record_start": format!("{name}-pinch") }), touch("down", 0, [c[0] - radius, c[1]]), touch("down", 1, [c[0] + radius, c[1]])];
        for i in 1..=10 {
            let s = i as f32 * 5.;
            events.push(touch("move", 0, [c[0] - radius - s, c[1]]));
            events.push(touch("move", 1, [c[0] + radius + s, c[1]]));
        }
        events.extend([serde_json::json!({ "touch": "up", "slot": 0 }), serde_json::json!({ "touch": "up", "slot": 1 }), serde_json::json!({ "wait_ms": 600 }), serde_json::json!({ "record_stop": true })]);
        d.input.perform(serde_json::Value::Array(events));
        pump(300);
        assert!(find_id(&d, ring.id).unwrap().diameter > ring.diameter, "{name}: the pinch resized the ring");
        let ring = find_id(&d, ring.id).unwrap();
        let from = rim(&d, &ring, 200.);
        let mut events = vec![serde_json::json!({ "record_start": format!("{name}-putback") }), contact("mouse", "down", from)];
        events.extend([[from[0], from[1] - 20.], [from[0], 10.]].map(|p| contact("mouse", "move", p)));
        events.extend([contact("mouse", "up", [from[0], 10.]), serde_json::json!({ "wait_ms": 500 }), serde_json::json!({ "record_stop": true })]);
        d.input.perform(serde_json::Value::Array(events));
        pump(300);
        assert!(views(&d).is_empty(), "{name}: the ring went back");
        if let Some(dir) = &captures {
            for part in ["tear", "pinch", "putback"] {
                let dark = dark_outside_rings(&dir.join(format!("{name}-{part}")));
                assert!(dark.is_empty(), "{name}-{part}: dark pixels outside the ring in frames {dark:?}");
            }
        }
    }
    d.finish();
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_dither_cell_size_bubble"]
fn native_dither_cell_size_bubble() {
    cell_size_bubble(layer_core::DefaultBrushPreset::DitherBlender, "dither_cell_size", "dither", "art.capycanvas.DitherCellSizeBubble");
}

#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_scatter_cell_size_bubble"]
fn native_scatter_cell_size_bubble() {
    cell_size_bubble(layer_core::DefaultBrushPreset::ScatterBlender, "scatter_cell_size", "scatter", "art.capycanvas.ScatterCellSizeBubble");
}

fn cell_size_bubble(preset: layer_core::DefaultBrushPreset, cell_id: &str, name: &str, application_id: &str) {
    let (default_spread, default_cell) = if preset == layer_core::DefaultBrushPreset::ScatterBlender { (0.2, 3.) } else { (0.08, 4.) };
    let mut d = Driver::new(application_id);
    for theme in [Theme::Light, Theme::Dark] {
        for device in ["mouse", "touch"] {
            d.w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }),
            });
            d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            d.w.dispatch(UiAction::SelectBrush { id: preset as u32 });
            d.w.dispatch(UiAction::ResetToolSetting { id: cell_id.into() });
            d.w.dispatch(UiAction::ResetToolSetting { id: "spread".into() });
            pump(300);
            let spot = canvas_spot(&d, 0.45, 0.5);
            let id = take(&mut d, BubbleGrab::Variant { variant: ToolVariant::BrushPreset { id: preset as u32 } }, spot);
            let bubble = find_id(&d, id).unwrap();
            assert!(bubble.selected && bubble.open);
            assert_eq!(ui_session(&d.w).engine().configured_brush().blender.reach, default_spread);
            assert_eq!(ui_session(&d.w).engine().configured_brush().blender.cell_size, default_cell);
            assert_eq!(bubble.rings.iter().map(|ring| ring.setting).collect::<Vec<_>>(), if preset == layer_core::DefaultBrushPreset::ScatterBlender { vec!["spread", cell_id, "grit"] } else { vec!["spread", cell_id] });
            let ring = bubble.rings.iter().find(|ring| ring.setting == cell_id).unwrap();
            let spread = bubble.rings.iter().find(|ring| ring.setting == "spread").unwrap();
            if preset == layer_core::DefaultBrushPreset::DitherBlender { assert!(spread.arc.point(0.5)[0] < 0. && ring.arc.point(0.5)[0] > 0.); }
            let at = center(&d, id);
            let spread_arc = |fraction| { let point = spread.arc.point(fraction); [at[0] + point[0], at[1] + point[1]] };
            for (fraction, expected) in [(0.5, 0.09), (1., 1.), (0., 0.)] {
                stroke(&mut d, device, &[spread_arc(0.2), spread_arc(fraction)]);
                let value = ui_session(&d.w).engine().configured_brush().blender.reach;
                assert!((value - expected).abs() < 0.011, "{theme:?}/{device}: Spread at {fraction}: {value}");
            }
            capture(&mut d, &format!("{name}-spread-{theme:?}-{device}").to_lowercase());
            let arc = |fraction| { let point = ring.arc.point(fraction); [at[0] + point[0], at[1] + point[1]] };
            let value = |d: &Driver| ui_session(&d.w).engine().configured_brush().blender.cell_size;
            assert_eq!(value(&d), default_cell);
            stroke(&mut d, device, &[arc(0.1), arc(0.5)]);
            let midpoint = value(&d);
            assert_eq!(midpoint.fract(), 0.);
            assert!((2. ..8.).contains(&midpoint), "{theme:?}/{device}: nonlinear slider emphasizes small cells: {midpoint}");
            stroke(&mut d, device, &[arc(0.5), arc(1.)]);
            assert_eq!(value(&d), 50., "{theme:?}/{device}: maximum cell size");
            capture(&mut d, &format!("{name}-cell-size-{theme:?}-{device}-50").to_lowercase());
            stroke(&mut d, device, &[arc(0.8), arc(0.)]);
            assert_eq!(value(&d), 1., "{theme:?}/{device}: minimum cell size");
            capture(&mut d, &format!("{name}-cell-size-{theme:?}-{device}-1").to_lowercase());
        }
    }
    assert!(state(&d.w).host_error.is_none());
    d.finish();
}

fn native_blender_profiles() -> Vec<(layer_core::DefaultBrushPreset, &'static [&'static str])> {
    use layer_core::DefaultBrushPreset::*;
    vec![
        (Smudge, &["pull", "spread"]), (NaturalBlender, &["pull", "spread"]),
        (SoftBlender, &["pull", "spread"]), (BlendingStump, &["pull", "spread"]), (Tissue, &["pull", "spread"]),
        (Blur, &["spread"]), (GrainyBlender, &["spread", "grain_depth"]),
        (BristleBlender, &["pull", "streaks", "spread"]), (BlendingKnife, &["pull", "streaks"]),
        (TextureBlender, &["texture_kept", "detail_size", "spread"]), (HueBlender, &["blend_lightness", "spread"]),
        (WetBlender, &["pull", "dilution", "average_pickup"]), (ColorMixer, &["pull", "average_pickup", "persistence"]),
        (PainterlyBlender, &["spread", "stretch"]), (CombBlender, &["edge_keeping", "spread"]),
        (ScatterBlender, &["spread", "scatter_cell_size", "grit"]), (Sharpen, &["sharpness", "spread"]),
        (Marbling, &["swirl", "swirl_size"]), (CrystalBlender, &["cell_size", "stretch", "spread"]),
        (DitherBlender, &["spread", "dither_cell_size"]),
    ]
}


#[test]
#[ignore = "private display: workspace-motion.sh gtk --native-test=native_blender_control_profiles"]
fn native_blender_control_profiles() {
    let mut d = Driver::new("art.capycanvas.BlenderControlProfiles");
    for theme in [Theme::Light, Theme::Dark] {
        for (preset, expected) in native_blender_profiles() {
            let mut workspace = WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() };
            workspace.layout.set_panel_visible(Panel::ToolSettings, true).unwrap();
            workspace.layout.move_panel([1600., 1000.], Panel::ToolSettings, DockTarget::Edge { edge: Edge::Right, outer: false }).unwrap();
            d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
            d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            d.w.dispatch(UiAction::SelectBrush { id: preset as u32 });
            pump(150);
            let settings = state(&d.w).tool_settings;
            let primary: Vec<_> = settings.iter().filter(|c| c.group_id.is_empty() && !matches!(c.id, "size" | "flow" | "strength" | "smoothing_strength")).map(|c| c.id).collect();
            assert_eq!(primary, expected, "{theme:?}/{preset:?}");
            let panel = d.w.panel_widget(Panel::ToolSettings);
            let rendered: Vec<_> = widgets(&panel).filter(|widget| widget.widget_name().starts_with("tool-setting-") && widget.is::<crate::number_control::NumberControl>()).map(|widget| widget.widget_name().trim_start_matches("tool-setting-").to_owned()).collect();
            assert_eq!(rendered, settings.iter().map(|c| c.id).collect::<Vec<_>>(), "{theme:?}/{preset:?}: native panel order");
            let spot = canvas_spot(&d, 0.4, 0.5);
            let id = take(&mut d, BubbleGrab::Variant { variant: ToolVariant::BrushPreset { id: preset as u32 } }, spot);
            let bubble = find_id(&d, id).unwrap();
            assert_eq!(bubble.rings.iter().map(|r| r.setting).collect::<Vec<_>>(), expected, "{theme:?}/{preset:?}: default bubble");
            if matches!(preset, layer_core::DefaultBrushPreset::BristleBlender | layer_core::DefaultBrushPreset::GrainyBlender) {
                capture(&mut d, &format!("blender-profile-initial-{preset:?}-{theme:?}").to_lowercase());
            }
            let ring = &bubble.rings[0];
            let at = center(&d, id);
            let arc = |fraction| { let point = ring.arc.point(fraction); [at[0] + point[0], at[1] + point[1]] };
            stroke(&mut d, "mouse", &[arc(0.2), arc(0.6)]);
            let actual = state(&d.w).tool_settings.into_iter().find(|c| c.id == ring.setting).unwrap();
            let target = actual.numeric.resolve(0., NumericOperation::Position { position: 0.6 }).unwrap().value as f32;
            assert!((actual.value - target).abs() < 0.011, "{theme:?}/{preset:?}: native halo adjustment");
            capture(&mut d, &format!("blender-profile-{preset:?}-{theme:?}").to_lowercase());
        }
    }
    assert!(state(&d.w).host_error.is_none());
    d.finish();
}
