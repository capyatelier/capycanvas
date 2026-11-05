//! GTK vector layer edges with actual compositor contacts: mouse, touch and the
//! pen's eraser end, refusal cursors, touch navigation during strokes and drags,
//! keys during drags, notices, long German labels, layer rows and saving.
use super::*;
use super::selection_tools::canvas_point;
use super::vector_layers::{active_vector, add_vector_layer, capture, paths, press_notice, start, tool, tool_opener, until_paths, wait_for};
use layer_core::{DefaultBrushPreset, FigurePaint, FigureShape};
use layer_ui::{NoticeActionId, RegionSource};

fn contact_script(device: &str, points: &[[f32; 2]]) -> serde_json::Value {
    let (first, last) = (points[0], points[points.len() - 1]);
    let mut script = match device {
        "mouse" => vec![serde_json::json!({"point":first}), serde_json::json!({"down":true})],
        "touch" => vec![serde_json::json!({"touch":"down","point":first})],
        tool => vec![serde_json::json!({"pen":"move","point":first,"tool":tool}), serde_json::json!({"pen":"down","point":first,"tool":tool})],
    };
    script.extend(points[1..].iter().map(|p| match device {
        "mouse" => serde_json::json!({"point":p}),
        "touch" => serde_json::json!({"touch":"move","point":p}),
        tool => serde_json::json!({"pen":"move","point":p,"tool":tool}),
    }));
    script.extend(match device {
        "mouse" => vec![serde_json::json!({"down":false})],
        "touch" => vec![serde_json::json!({"touch":"up","point":last})],
        tool => vec![serde_json::json!({"pen":"up","tool":tool}), serde_json::json!({"pen":"leave","tool":tool})],
    });
    serde_json::Value::Array(script)
}
fn draw(d: &mut Driver, device: &str, points: &[[f32; 2]]) {
    let points: Vec<_> = points.iter().map(|p| canvas_point(d, *p)).collect();
    d.input.perform(contact_script(device, &points));
}
fn darkest(d: &Driver, at: [f32; 2], radius: i32) -> [u8; 4] {
    let capture = ui_session(&d.w).engine().backend().capture_in(d.w.view_color()).unwrap();
    let m = state(&d.w).camera.document_to_surface();
    let cx = (m[0] * at[0] + m[2] * at[1] + m[4]).round() as i32;
    let cy = (m[1] * at[0] + m[3] * at[1] + m[5]).round() as i32;
    let mut best = [255; 4];
    for y in (cy - radius).max(0)..=(cy + radius).min(capture.height as i32 - 1) {
        for x in (cx - radius).max(0)..=(cx + radius).min(capture.width as i32 - 1) {
            let pixel: [u8; 4] = capture.bytes[y as usize * capture.stride as usize + x as usize * 4..][..4].try_into().unwrap();
            if pixel[..3].iter().map(|v| u16::from(*v)).sum::<u16>() < best[..3].iter().map(|v| u16::from(*v)).sum::<u16>() { best = pixel; }
        }
    }
    best
}
fn ink_at(d: &Driver, at: [f32; 2]) -> bool { darkest(d, at, 4)[..3].iter().any(|v| *v < 110) }
fn expect_ink(d: &mut Driver, what: &str, at: [f32; 2], wanted: bool) {
    d.input.perform(serde_json::json!([{"point":[4.,4.]}]));
    let deadline = Instant::now() + Duration::from_secs(20);
    while ink_at(d, at) != wanted {
        pump(30);
        assert!(Instant::now() < deadline, "{what}: wanted ink={wanted} at {at:?}, darkest {:?}", darkest(d, at, 4));
    }
}
fn selected_items(d: &Driver) -> Vec<layer_core::authored::vector::ItemId> {
    ui_session(&d.w).engine().document().working.vector_items.as_ref().map_or_else(Vec::new, |s| s.items.iter().copied().collect())
}
fn refusal_cursor(d: &mut Driver, device: &str, at: [f32; 2]) -> bool {
    let p = canvas_point(d, at);
    let nudge = [p[0] + 3., p[1] + 2.];
    d.input.perform(match device {
        "mouse" => serde_json::json!([{"point":p},{"point":nudge}]),
        _ => serde_json::json!([{"pen":"move","point":p},{"pen":"move","point":nudge}]),
    });
    pump(120);
    let refused = ui_session_mut(&d.w).canvas_cursor().is_some_and(|c| c.segments.iter().any(|segment| segment.marker == 6.));
    if device != "mouse" { d.input.perform(serde_json::json!([{"pen":"leave"}])); }
    refused
}
fn theme_name() -> String { std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "light".into()) }
fn line_points(from: [f32; 2], to: [f32; 2], steps: usize) -> Vec<[f32; 2]> {
    (0..=steps).map(|i| { let u = i as f32 / steps as f32; [from[0] + (to[0] - from[0]) * u, from[1] + (to[1] - from[1]) * u] }).collect()
}

fn camera_key(d: &Driver) -> [f32; 6] { state(&d.w).camera.document_to_surface() }
fn report(findings: &[String]) {
    assert!(findings.is_empty(), "findings:\n{}", findings.join("\n"));
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_input_kinds --tablet"]
fn native_vector_input_kinds() {
    let mut d = Driver::new("art.capycanvas.VectorInputKinds");
    let theme = theme_name();
    let mut findings = Vec::new();
    d.w.dispatch(UiAction::SetColor { rgba: [0.1, 0.1, 0.12, 1.] });
    add_vector_layer(&mut d);
    d.w.dispatch(UiAction::SetBrushSize { value: 14. });
    pump(150);
    let rows = [300., 450., 600.];
    for (index, device) in ["pen", "mouse"].into_iter().enumerate() {
        draw(&mut d, device, &line_points([400., rows[index]], [1200., rows[index] + 30.], 24));
        until_paths(&d, &format!("a {device} line on the vector layer"), |paths| paths.len() == index + 1);
        expect_ink(&mut d, &format!("{theme} {device} line"), [800., rows[index] + 15.], true);
    }
    let camera = camera_key(&d);
    draw(&mut d, "touch", &line_points([400., rows[2]], [700., rows[2] + 30.], 12));
    pump(300);
    assert_eq!(paths(&d).len(), 2, "one finger never draws");
    assert_eq!(camera_key(&d), camera, "one finger never pans");
    draw(&mut d, "pen", &line_points([400., rows[2]], [1200., rows[2] + 30.], 24));
    until_paths(&d, "the third pen line", |paths| paths.len() == 3);
    capture(&d, &format!("input-lines-{theme}.png"));

    let before = paths(&d);
    d.w.dispatch(UiAction::SetBrushSize { value: 60. });
    pump(100);
    draw(&mut d, "eraser", &line_points([900., 200.], [905., 700.], 20));
    until_paths(&d, "the pen's eraser end cuts every line", |paths| paths.len() == 2 * before.len());
    for row in rows { expect_ink(&mut d, &format!("{theme} erased at {row}"), [902., row + 19.], false); }
    expect_ink(&mut d, &format!("{theme} kept left of the eraser"), [600., rows[0] + 8.], true);
    assert_eq!(state(&d.w).brush.preset, DefaultBrushPreset::GPen as u32, "the eraser end leaves G-Pen chosen");
    capture(&d, &format!("input-eraser-end-{theme}.png"));
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until_paths(&d, "undoing the eraser end", |paths| paths.len() == before.len());
    d.w.dispatch(UiAction::SetBrushSize { value: 14. });

    let empty = [1400., 850.];
    type CursorCase = (&'static str, Box<dyn Fn(&Driver)>, bool);
    let cursors: [CursorCase; 13] = [
        ("G-Pen", Box::new(|d| d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 })), false),
        ("Monoline", Box::new(|d| d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Monoline as u32 })), false),
        ("Pencil", Box::new(|d| d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Pencil as u32 })), true),
        ("Smudge", Box::new(|d| d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Smudge as u32 })), true),
        ("Eraser", Box::new(|d| d.w.dispatch(UiAction::Invoke { command: CommandId::Eraser })), false),
        ("Figure", Box::new(|d| tool(d, LayerCanvasTool::Figure { shape: FigureShape::Star, paint: FigurePaint::Both })), false),
        ("Fill", Box::new(|d| tool(d, LayerCanvasTool::Region { fill: true, source: RegionSource::Visible })), false),
        ("Lasso Fill", Box::new(|d| tool(d, LayerCanvasTool::LassoFill)), false),
        ("Enclose Fill", Box::new(|d| tool(d, LayerCanvasTool::EncloseFill { source: RegionSource::Visible })), false),
        ("Gradient", Box::new(|d| tool(d, LayerCanvasTool::Gradient { shape: layer_core::GradientShape::Linear })), true),
        ("Move", Box::new(|d| d.w.dispatch(UiAction::Invoke { command: CommandId::Move })), false),
        ("Edit Points", Box::new(|d| d.w.dispatch(UiAction::Invoke { command: CommandId::EditPoints })), false),
        ("Lasso", Box::new(|d| tool(d, LayerCanvasTool::Select)), false),
    ];
    for (name, choose, refused) in &cursors {
        choose(&d);
        pump(200);
        for device in ["pen", "mouse"] {
            let seen = refusal_cursor(&mut d, device, empty);
            if seen != *refused { findings.push(format!("{theme} {name} by {device}: refusal cursor {seen}, expected {refused}")); }
        }
        if *name == "Gradient" { capture(&d, &format!("cursor-refused-{theme}.png")); }
    }
    d.input.perform(serde_json::json!([{"point":[4.,4.]}]));

    d.w.dispatch(UiAction::Invoke { command: CommandId::Move });
    pump(200);
    let lines = paths(&d);
    for (index, device) in ["pen", "mouse", "touch"].into_iter().enumerate() {
        let path = lines.iter().find(|p| (start(p)[1] - rows[index]).abs() < 4.).unwrap_or_else(|| panic!("line {index}")).clone();
        let [x, y] = start(&path);
        let grab = [x + 200., y + 7.5];
        let camera = camera_key(&d);
        draw(&mut d, device, &line_points(grab, [grab[0] + 60., grab[1] + 50.], 8));
        let deadline = Instant::now() + Duration::from_secs(5);
        let dragged = loop {
            pump(30);
            if paths(&d).iter().any(|p| p.id == path.id && (start(p)[0] - x - 60.).abs() < 3. && (start(p)[1] - y - 50.).abs() < 3.) { break true; }
            if Instant::now() > deadline { break false; }
        };
        if !dragged || selected_items(&d) != vec![path.id] {
            findings.push(format!("{theme} Move by {device}: dragged {dragged}, selection {:?}, view moved {}", selected_items(&d), camera_key(&d) != camera));
            d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
            pump(300);
        }
    }
    capture(&d, &format!("input-dragged-{theme}.png"));

    d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 });
    pump(200);
    let count = paths(&d).len();
    let camera = camera_key(&d);
    let a: Vec<_> = line_points([400., 800.], [1000., 830.], 16).into_iter().map(|p| canvas_point(&d, p)).collect();
    let (f, g) = (canvas_point(&d, [1200., 500.]), canvas_point(&d, [1400., 600.]));
    let mut script = vec![serde_json::json!({"pen":"move","point":a[0]}), serde_json::json!({"pen":"down","point":a[0]})];
    for (i, p) in a[1..].iter().enumerate() {
        script.push(serde_json::json!({"pen":"move","point":p}));
        match i {
            4 => script.extend([serde_json::json!({"touch":"down","point":f}), serde_json::json!({"touch":"down","point":g,"slot":1})]),
            5..=9 => { let k = (i - 4) as f32 * 30.; script.extend([serde_json::json!({"touch":"move","point":[f[0] + k, f[1] - k]}), serde_json::json!({"touch":"move","point":[g[0] + 2. * k, g[1] - k],"slot":1})]); }
            10 => script.extend([serde_json::json!({"touch":"up","point":f}), serde_json::json!({"touch":"up","point":g,"slot":1})]),
            _ => {}
        }
    }
    script.extend([serde_json::json!({"pen":"up"}), serde_json::json!({"pen":"leave"})]);
    d.input.perform(serde_json::Value::Array(script));
    until_paths(&d, "the pen line drawn under two fingers", |paths| paths.len() == count + 1);
    if camera_key(&d) != camera { findings.push(format!("{theme} two fingers moved the view while the pen drew")); d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas }); pump(300); }
    expect_ink(&mut d, &format!("{theme} the line keeps its end after the gesture"), [980., 829.], true);
    capture(&d, &format!("input-two-finger-stroke-{theme}.png"));

    d.w.dispatch(UiAction::Invoke { command: CommandId::Move });
    pump(300);
    let path = paths(&d).into_iter().find(|p| (start(p)[1] - 800.).abs() < 4.).unwrap();
    let [x, y] = start(&path);
    let camera = camera_key(&d);
    let a: Vec<_> = line_points([x + 300., y + 11.], [x + 300., y + 131.], 12).into_iter().map(|p| canvas_point(&d, p)).collect();
    let mut script = vec![serde_json::json!({"pen":"move","point":a[0]}), serde_json::json!({"pen":"down","point":a[0]})];
    for (i, p) in a[1..].iter().enumerate() {
        script.push(serde_json::json!({"pen":"move","point":p}));
        match i {
            3 => script.extend([serde_json::json!({"touch":"down","point":f}), serde_json::json!({"touch":"down","point":g,"slot":1})]),
            4..=7 => { let k = (i - 3) as f32 * 40.; script.extend([serde_json::json!({"touch":"move","point":[f[0] - k, f[1] - k]}), serde_json::json!({"touch":"move","point":[g[0] + k, g[1] + k],"slot":1})]); }
            8 => script.extend([serde_json::json!({"touch":"up","point":f}), serde_json::json!({"touch":"up","point":g,"slot":1})]),
            _ => {}
        }
    }
    script.extend([serde_json::json!({"pen":"up"}), serde_json::json!({"pen":"leave"})]);
    d.input.perform(serde_json::Value::Array(script));
    until_paths(&d, "the pen drag under a pinch", |paths| paths.iter().any(|p| p.id == path.id && (start(p)[1] - y - 120.).abs() < 3.));
    if camera_key(&d) != camera { findings.push(format!("{theme} a pinch zoomed the view while the pen dragged a path")); }
    capture(&d, &format!("input-two-finger-drag-{theme}.png"));
    report(&findings);
    d.finish();
}

const CONTROL: u32 = 0xffe3;
fn chord(d: &mut Driver, modifiers: &[u32], key: u32) { d.input.perform(serde_json::Value::Array(key_events(modifiers, key))); }
fn key_events(modifiers: &[u32], key: u32) -> Vec<serde_json::Value> {
    let mut events: Vec<_> = modifiers.iter().map(|m| serde_json::json!({"key":m,"down":true})).collect();
    events.extend([serde_json::json!({"key":key,"down":true}), serde_json::json!({"key":key,"down":false})]);
    events.extend(modifiers.iter().rev().map(|m| serde_json::json!({"key":m,"down":false})));
    events
}
fn canvas_tool(d: &Driver) -> LayerCanvasTool { state(&d.w).layer_tools.tool }
fn checkpoint(d: &Driver) -> u64 { ui_session(&d.w).engine().checkpoint() }
fn path_by_row(d: &Driver, row: f32) -> layer_core::authored::vector::PathItem {
    paths(d).into_iter().find(|p| (start(p)[1] - row).abs() < 4.).unwrap_or_else(|| panic!("no path starts at row {row}: {:?}", paths(d).iter().map(start).collect::<Vec<_>>()))
}
fn mouse_drag_with(d: &mut Driver, points: &[[f32; 2]], at: usize, keys: Vec<serde_json::Value>) {
    let points: Vec<_> = points.iter().map(|p| canvas_point(d, *p)).collect();
    let mut script = vec![serde_json::json!({"point":points[0]}), serde_json::json!({"down":true})];
    for (i, p) in points[1..].iter().enumerate() {
        script.push(serde_json::json!({"point":p}));
        if i + 1 == at { script.push(serde_json::json!({"wait_ms":150})); script.extend(keys.iter().cloned()); script.push(serde_json::json!({"wait_ms":150})); }
    }
    script.push(serde_json::json!({"down":false}));
    d.input.perform(serde_json::Value::Array(script));
}
fn vector_layers(d: &Driver) -> usize { state(&d.w).layers.iter().filter(|l| l.content_icon.as_deref() == Some("layer-path-symbolic")).count() }

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_keys_and_touch_points --tablet"]
fn native_vector_keys_and_touch_points() {
    let mut d = Driver::new("art.capycanvas.VectorKeys");
    let theme = theme_name();
    let mut findings = Vec::new();
    d.w.dispatch(UiAction::SetColor { rgba: [0.1, 0.1, 0.12, 1.] });
    add_vector_layer(&mut d);
    d.w.dispatch(UiAction::SetBrushSize { value: 14. });
    pump(150);
    let rows = [300., 500., 700.];
    for (index, row) in rows.into_iter().enumerate() {
        draw(&mut d, "pen", &line_points([400., row], [1200., row], 24));
        until_paths(&d, "a G-Pen line", |paths| paths.len() == index + 1);
    }

    chord(&mut d, &[CONTROL], 0x61);
    wait_for("Ctrl+A selects every path and switches to Move", || selected_items(&d).len() == 3 && canvas_tool(&d) == LayerCanvasTool::Move);
    capture(&d, &format!("keys-select-all-{theme}.png"));
    d.input.key(0xff1b);
    wait_for("Escape clears the path selection", || selected_items(&d).is_empty());

    let first = path_by_row(&d, rows[0]);
    let p = canvas_point(&d, [800., rows[0]]);
    d.input.click(p);
    wait_for("a click selects the path", || selected_items(&d) == vec![first.id]);
    d.input.key(0xff0d);
    wait_for("Enter opens Edit Points", || canvas_tool(&d) == LayerCanvasTool::EditPoints);
    capture(&d, &format!("keys-edit-points-{theme}.png"));
    d.input.key(0xff1b);
    wait_for("Escape returns to Move with the path still selected", || canvas_tool(&d) == LayerCanvasTool::Move && selected_items(&d) == vec![first.id]);
    d.input.key(0xffff);
    until_paths(&d, "Delete removes the selected path", |paths| paths.len() == 2 && paths.iter().all(|p| p.id != first.id));
    expect_ink(&mut d, &format!("{theme} deleted line"), [800., rows[0]], false);
    chord(&mut d, &[CONTROL], 0x7a);
    until_paths(&d, "Ctrl+Z restores the path", |paths| paths.len() == 3);
    expect_ink(&mut d, &format!("{theme} restored line"), [800., rows[0]], true);

    let second = path_by_row(&d, rows[1]);
    let before = checkpoint(&d);
    mouse_drag_with(&mut d, &line_points([800., rows[1]], [800., rows[1] + 120.], 6), 6, key_events(&[], 0xff1b));
    pump(300);
    let kept = paths(&d).into_iter().find(|p| p.id == second.id).unwrap();
    if start(&kept) != start(&second) || checkpoint(&d) != before { findings.push(format!("{theme} Escape mid-drag: path start {:?} (was {:?}), history changed {}", start(&kept), start(&second), checkpoint(&d) != before)); }
    expect_ink(&mut d, &format!("{theme} Escape leaves the path in place"), [800., rows[1]], true);
    expect_ink(&mut d, &format!("{theme} Escape removes the drag preview"), [800., rows[1] + 120.], false);
    capture(&d, &format!("keys-escape-drag-{theme}.png"));

    let before = checkpoint(&d);
    mouse_drag_with(&mut d, &line_points([800., rows[1]], [800., rows[1] + 100.], 5), 5, key_events(&[CONTROL], 0x7a));
    let moved = until_paths(&d, "the drag that saw Ctrl+Z commits", |paths| paths.iter().any(|p| p.id == second.id && (start(p)[1] - rows[1] - 100.).abs() < 3.));
    if moved.len() != 3 { findings.push(format!("{theme} Ctrl+Z during a path drag undid an earlier step: {} paths remain", moved.len())); }
    assert_ne!(checkpoint(&d), before, "the drag is one undo step");
    chord(&mut d, &[CONTROL], 0x7a);
    until_paths(&d, "Ctrl+Z after the drag undoes it", |paths| paths.iter().any(|p| p.id == second.id && start(p) == start(&second)));
    if paths(&d).len() < 3 {
        d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 });
        pump(200);
        draw(&mut d, "pen", &line_points([400., rows[2]], [1200., rows[2]], 24));
        until_paths(&d, "the third line again", |paths| paths.len() == 3);
        d.w.dispatch(UiAction::Invoke { command: CommandId::Move });
        pump(200);
    }

    d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 });
    pump(200);
    let points: Vec<_> = line_points([400., 900.], [1200., 900.], 20).into_iter().map(|p| canvas_point(&d, p)).collect();
    let mut script = vec![serde_json::json!({"pen":"move","point":points[0]}), serde_json::json!({"pen":"down","point":points[0]})];
    for (i, p) in points[1..].iter().enumerate() {
        script.push(serde_json::json!({"pen":"move","point":p}));
        if i == 8 { script.extend(key_events(&[CONTROL], 0x7a)); }
    }
    script.extend([serde_json::json!({"pen":"up"}), serde_json::json!({"pen":"leave"})]);
    let earlier = paths(&d);
    d.input.perform(serde_json::Value::Array(script));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !paths(&d).iter().any(|p| (start(p)[1] - 900.).abs() < 4.) && Instant::now() < deadline { pump(50); }
    let after = paths(&d);
    capture(&d, &format!("keys-undo-mid-stroke-{theme}.png"));
    let lost: Vec<_> = earlier.iter().filter(|p| !after.iter().any(|q| q.id == p.id)).map(|p| start(p)[1]).collect();
    let kept = after.iter().any(|p| (start(p)[1] - 900.).abs() < 4.);
    if !lost.is_empty() || !kept {
        findings.push(format!("{theme} Ctrl+Z during a G-Pen stroke: stroke kept {kept}, earlier lines lost at {lost:?}"));
        for row in lost.into_iter().chain((!kept).then_some(900.)) {
            draw(&mut d, "pen", &line_points([400., row], [1200., row], 24));
            until_paths(&d, "redrawing the line", |paths| paths.iter().any(|p| (start(p)[1] - row).abs() < 4.));
        }
    }
    expect_ink(&mut d, &format!("{theme} the stroke keeps its end"), [1180., 900.], true);

    d.w.dispatch(UiAction::Invoke { command: CommandId::Move });
    pump(200);
    let third = path_by_row(&d, rows[2]);
    d.input.click(canvas_point(&d, [800., rows[2]]));
    wait_for("a click selects the third path", || selected_items(&d) == vec![third.id]);
    d.w.dispatch(UiAction::Invoke { command: CommandId::EditPoints });
    pump(200);
    let anchor = start(&third);
    draw(&mut d, "touch", &line_points(anchor, [anchor[0], anchor[1] + 120.], 8));
    pump(300);
    let touched = paths(&d).into_iter().find(|p| p.id == third.id).unwrap();
    if (start(&touched)[1] - anchor[1] - 120.).abs() > 3. { findings.push(format!("{theme} Edit Points by touch: dragging the first anchor left it at {:?}", start(&touched))); }
    let anchors = touched.geometry.anchor_count();
    let ends = touched.geometry.subpaths()[0].clone();
    let [a, b] = [ends.anchor(0).point(), ends.anchor(1).point()];
    let tap = canvas_point(&d, [((a[0] + b[0]) * 0.5) as f32, ((a[1] + b[1]) * 0.5) as f32]);
    d.input.perform(serde_json::json!([{"touch":"down","point":tap},{"touch":"up","point":tap}]));
    pump(300);
    let tapped = paths(&d).into_iter().find(|p| p.id == third.id).unwrap();
    if tapped.geometry.anchor_count() != anchors + 1 { findings.push(format!("{theme} Edit Points by touch: tapping a segment left {} anchors (was {anchors})", tapped.geometry.anchor_count())); }
    capture(&d, &format!("keys-touch-points-{theme}.png"));

    d.w.dispatch(UiAction::Invoke { command: CommandId::Move });
    let lower = paths(&d).len();
    d.w.dispatch(UiAction::Layer { action: LayerAction::NewVector });
    wait_for("a second vector layer", || vector_layers(&d) == 2 && paths(&d).is_empty());
    d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 });
    pump(200);
    draw(&mut d, "pen", &line_points([400., 1000.], [1200., 1000.], 24));
    until_paths(&d, "a line on the upper vector layer", |paths| paths.len() == 1);
    chord(&mut d, &[CONTROL], 0x65);
    wait_for("Ctrl+E merges the vector layers as paths", || vector_layers(&d) == 1 && paths(&d).len() == lower + 1);
    assert!(state(&d.w).notice.is_none(), "merging vector layers asks nothing");
    chord(&mut d, &[CONTROL], 0x65);
    wait_for("Ctrl+E onto paint asks first", || state(&d.w).notice.as_ref().is_some_and(|n| n.actions.iter().any(|a| a.id == layer_ui::NoticeActionId::Merge)));
    capture(&d, &format!("keys-merge-notice-{theme}.png"));
    press_notice(&mut d, layer_ui::NoticeActionId::Merge);
    wait_for("the merge bakes the paths", || vector_layers(&d) == 0);
    expect_ink(&mut d, &format!("{theme} merged lines"), [800., 1000.], true);
    expect_ink(&mut d, &format!("{theme} merged lines keep the earlier ones"), [800., rows[0]], true);
    capture(&d, &format!("keys-merged-{theme}.png"));
    report(&findings);
    d.finish();
}

fn window_pixel(d: &Driver, at: [f32; 2]) -> [u8; 4] {
    let texture = crate::snapshot(&d.w);
    let mut downloader = gdk::TextureDownloader::new(&texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let scale = texture.width() as f32 / d.w.window.width() as f32;
    let [x, y] = at.map(|v| (v * scale).round() as usize);
    bytes[y * stride + x * 4..][..4].try_into().unwrap()
}
fn clipped(widget: &gtk::Widget, within: &gtk::Widget, root: &gtk::Widget) -> Option<String> {
    let b = widget.compute_bounds(root)?;
    let c = within.compute_bounds(root)?;
    let name = widget.downcast_ref::<gtk::Label>().map_or_else(|| widget.widget_name().to_string(), |l| l.text().to_string());
    if b.x() < c.x() - 1. || b.y() < c.y() - 1. || b.x() + b.width() > c.x() + c.width() + 1. || b.y() + b.height() > c.y() + c.height() + 1. {
        return Some(format!("{name} at {:?} leaves {} at {:?}", [b.x(), b.y(), b.width(), b.height()], within.widget_name(), [c.x(), c.y(), c.width(), c.height()]));
    }
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        let minimum = label.measure(gtk::Orientation::Horizontal, -1).0;
        if label.layout().is_ellipsized() || label.width() < minimum { return Some(format!("{name} is cut to {} px", label.width())); }
    }
    None
}
fn footer_findings(d: &Driver, place: &str) -> Vec<String> {
    let root: gtk::Widget = d.w.window.clone().upcast();
    let button = widgets(&root).find(|w| w.widget_name() == "layer-add-vector-layer-symbolic" && w.is_mapped() && w.width() > 0).expect("a mapped New vector layer button");
    let footer = button.parent().unwrap();
    let page = std::iter::successors(footer.parent(), |w| w.parent()).find(|w| w.is::<gtk::Stack>() || w.widget_name().starts_with("drawer-panel")).unwrap_or_else(|| root.clone());
    let mut found = Vec::new();
    let mut child = footer.first_child();
    let mut previous: Option<gtk::graphene::Rect> = None;
    while let Some(widget) = child {
        child = widget.next_sibling();
        if !widget.is_visible() { continue; }
        if let Some(problem) = clipped(&widget, &footer, &root) { found.push(format!("{place} footer: {problem}")); }
        let bounds = widget.compute_bounds(&root).unwrap();
        if previous.is_some_and(|p| bounds.x() < p.x() + p.width() - 1.) { found.push(format!("{place} footer: {} overlaps the button before it", widget.widget_name())); }
        previous = Some(bounds);
    }
    if let Some(problem) = clipped(&footer, &page, &root) { found.push(format!("{place} footer: {problem}")); }
    if let Some(problem) = clipped(&footer, &root, &root) { found.push(format!("{place} footer: {problem}")); }
    let room = footer.parent().and_then(|viewport| viewport.parent()).unwrap_or_else(|| footer.clone());
    let overflow: Vec<String> = layer_ui::layer_footer_overflow(&state(&d.w).layer_tools.footer, room.width() as f32).into_iter().map(|item| item.label).collect();
    if !overflow.is_empty() {
        let more = widgets(&footer).find(|w| w.widget_name() == "layer-actions").and_then(|w| w.downcast::<gtk::Button>().ok()).expect("the Layers actions button");
        more.emit_clicked();
        wait_for("the Layers actions menu", || widgets(&root).any(|w| w.is::<gtk::PopoverMenu>() && w.is_mapped()));
        pump(200);
        let popover = widgets(&root).find_map(|w| w.downcast::<gtk::PopoverMenu>().ok().filter(|p| p.is_mapped())).unwrap();
        let shown: Vec<String> = widgets(popover.upcast_ref()).filter_map(|w| w.downcast::<gtk::Label>().ok()).map(|l| l.text().to_string()).collect();
        found.extend(overflow.iter().filter(|label| !shown.contains(label)).map(|label| format!("{place} footer: the actions menu does not list {label}")));
        popover.popdown();
        pump(150);
    }
    found
}
fn notice_findings(d: &Driver, place: &str) -> Vec<String> {
    let root: gtk::Widget = d.w.window.clone().upcast();
    let notice: gtk::Widget = d.w.notice.root.clone().upcast();
    let mut found = Vec::new();
    if !notice.is_mapped() { return vec![format!("{place} notice is not shown")]; }
    if let Some(problem) = clipped(&notice, &root, &root) { found.push(format!("{place} notice: {problem}")); }
    for widget in widgets(&notice).filter(|w| w.is_mapped() && (w.is::<gtk::Label>() || w.is::<gtk::Button>())) {
        if let Some(problem) = clipped(&widget, &notice, &root) { found.push(format!("{place} notice: {problem}")); }
        if let Some(button) = widget.downcast_ref::<gtk::Button>() && !button.is_focusable() { found.push(format!("{place} notice: {} cannot take keyboard focus", button.label().unwrap_or_default())); }
    }
    found
}
fn mode_findings(d: &Driver, place: &str) -> Vec<String> {
    let root: gtk::Widget = d.w.window.clone().upcast();
    let localization = d.w.localization();
    let mut found = Vec::new();
    for command in [CommandId::EraseTouchedPart, CommandId::EraseWholePaths, CommandId::EraseToIntersection] {
        let text = command.localized_label(localization.as_ref()).to_string();
        let labels: Vec<_> = widgets(&root).filter(|w| w.is_mapped() && w.width() > 0 && w.downcast_ref::<gtk::Label>().is_some_and(|l| l.text() == text)).collect();
        if labels.is_empty() { found.push(format!("{place}: no visible {text}")); }
        for label in labels {
        let within = std::iter::successors(label.parent(), |w| w.parent()).find(|w| w.is::<gtk::ScrolledWindow>() || w.widget_name().starts_with("drawer-panel")).unwrap_or_else(|| root.clone());
        if let Some(problem) = clipped(&label, &within, &root) { found.push(format!("{place} eraser mode: {problem}")); }
        let button = std::iter::successors(Some(label.clone()), |w| w.parent()).find(|w| w.is::<gtk::Button>()).unwrap_or(label.clone());
        if let Some(problem) = clipped(&label, &button, &root) { found.push(format!("{place} eraser mode button: {problem}")); }
        }
    }
    found
}
fn refuse_pencil(d: &mut Driver, row: f32) -> u64 {
    d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Pencil as u32 });
    pump(200);
    let [w, h] = ui_session(&d.w).engine().document().composition().size.map(|v| v as f32);
    draw(d, "mouse", &line_points([w * 0.3, h * row / 1000.], [w * 0.6, h * row / 1000.], 6));
    wait_for("the vector refusal", || state(&d.w).notice.as_ref().is_some_and(|n| n.actions.iter().any(|a| a.id == NoticeActionId::NewPaintLayer)) && d.w.notice.root.is_mapped());
    d.input.perform(serde_json::json!([{"point":[4.,4.]}]));
    state(&d.w).notice.unwrap().id
}
fn notice_button(d: &Driver, id: NoticeActionId) -> gtk::Button {
    let label = state(&d.w).notice.unwrap().actions.into_iter().find(|a| a.id == id).unwrap().label;
    widgets(d.w.notice.root.upcast_ref()).filter_map(|w| w.downcast::<gtk::Button>().ok()).find(|b| b.label().as_deref() == Some(label.as_str())).unwrap()
}
fn set_german(d: &Driver) {
    let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|l| *l == layer_ui::UiLanguage::German).unwrap() as u32;
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(choice) } });
    wait_for("German", || d.w.localization().language() == layer_ui::UiLanguage::German);
    pump(300);
}
fn open_drawer(d: &mut Driver, control: ToolbarControl, shown: impl Fn(&Driver) -> bool) -> Option<String> {
    if shown(d) { return None; }
    let opener = tool_opener(d, control);
    for _ in 0..3 {
        if shown(d) { return Some(opener); }
        if d.named(&opener).is_mapped() { d.click_name(&opener); } else { overflow_item(d, &opener); }
        let deadline = Instant::now() + Duration::from_millis(1500);
        while !shown(d) && Instant::now() < deadline {
            if state(&d.w).customization.drawer.as_ref().is_some_and(|drawer| drawer.layer_properties == Some(true)) { d.click_name("layer-properties-disclosure"); }
            pump(50);
        }
    }
    if shown(d) { return Some(opener); }
    capture(d, "drawer-failure.png");
    panic!("{opener} did not show its drawer");
}
fn overflow_item(d: &mut Driver, opener: &str) {
    let item = opener.replace("header-item-", "header-overflow-item-");
    for zone in 0..3 {
        let button = d.named(&format!("header-overflow-{zone}"));
        if !button.is_mapped() { continue; }
        d.click(&button);
        pump(300);
        let popover = button.downcast_ref::<gtk::MenuButton>().unwrap().popover().unwrap();
        if let Some(entry) = find_named(popover.upcast_ref(), &item).filter(|e| e.is_mapped()) {
            d.click(&entry);
            pump(300);
            return;
        }
        popover.popdown();
        pump(150);
    }
    panic!("no header overflow offers {item}");
}
fn footer_shown(d: &Driver) -> bool { widgets(d.w.window.upcast_ref()).any(|w| w.widget_name() == "layer-add-vector-layer-symbolic" && w.is_mapped() && w.width() > 0) }
fn modes_shown(d: &Driver) -> bool {
    let text = CommandId::EraseWholePaths.localized_label(d.w.localization().as_ref()).to_string();
    widgets(d.w.window.upcast_ref()).any(|w| w.is_mapped() && w.width() > 0 && w.downcast_ref::<gtk::Label>().is_some_and(|l| l.text() == text))
}
fn open_layers(d: &mut Driver) -> Option<String> { open_drawer(d, ToolbarControl::Panel { panel: Panel::Layers }, footer_shown) }
fn close_drawer(d: &mut Driver, opener: &Option<String>) {
    if state(&d.w).customization.drawer.is_some() {
        if let Some(opener) = opener.as_ref().filter(|name| d.named(name).is_mapped()) { d.click_name(opener); } else { d.input.key(0xff1b); }
    }
    pump(250);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_notices_and_german_labels"]
fn native_vector_notices_and_german_labels() {
    let mut d = Driver::new("art.capycanvas.VectorNotices");
    let theme = format!("{}-{}", theme_name(), d.w.window.width());
    let mut findings = Vec::new();
    set_german(&d);
    d.w.dispatch(UiAction::SetColor { rgba: [0.1, 0.1, 0.12, 1.] });
    let layers = open_layers(&mut d);
    findings.extend(footer_findings(&d, &format!("{theme} drawer")));
    capture(&d, &format!("de-footer-drawer-{theme}.png"));
    let button = widgets(d.w.window.upcast_ref()).find(|w| w.widget_name() == "layer-add-vector-layer-symbolic" && w.is_mapped() && w.width() > 0).unwrap();
    d.click(&button);
    wait_for("a vector layer from the drawer footer", || active_vector(&d));
    close_drawer(&mut d, &layers);
    let vector = state(&d.w).layers.into_iter().find(|l| l.selected).unwrap().id;

    let first = refuse_pencil(&mut d, 400.);
    findings.extend(notice_findings(&d, &format!("{theme} drawer workspace")));
    capture(&d, &format!("de-notice-{theme}.png"));
    let inside = { let b = d.w.notice.root.compute_bounds(&d.w.window).unwrap(); [b.x() + 6., b.y() + b.height() / 2.] };
    let before = window_pixel(&d, inside);
    let other = if theme_name() == "dark" { Theme::Light } else { Theme::Dark };
    d.w.dispatch(UiAction::SetTheme { theme: Some(other) });
    pump(250);
    let after = window_pixel(&d, inside);
    if !d.w.notice.root.is_mapped() || state(&d.w).notice.as_ref().map(|n| n.id) != Some(first) { findings.push(format!("{theme} switching the theme hid the notice")); }
    let luminance = |p: [u8; 4]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
    if (other == Theme::Dark) != (luminance(after) < luminance(before)) { findings.push(format!("{theme} the notice kept its colors after the theme switch: {before:?} -> {after:?}")); }
    findings.extend(notice_findings(&d, &format!("{theme} after the theme switch")));
    capture(&d, &format!("de-notice-switched-{theme}.png"));
    d.w.dispatch(UiAction::SetTheme { theme: Some(if theme_name() == "dark" { Theme::Dark } else { Theme::Light }) });
    pump(200);
    let rows = state(&d.w).layers.len();
    let button = notice_button(&d, NoticeActionId::NewPaintLayer);
    assert!(button.grab_focus(), "the notice's action takes keyboard focus");
    d.input.key(0xff0d);
    wait_for("Enter on the focused action adds a paint layer", || state(&d.w).layers.len() == rows + 1 && !active_vector(&d));

    d.w.dispatch(UiAction::Layer { action: LayerAction::Select { id: vector, mask: false } });
    wait_for("the vector layer again", || active_vector(&d));
    refuse_pencil(&mut d, 600.);
    let paint = state(&d.w).layers.into_iter().find(|l| !l.selected && l.content_icon.is_none() && l.paper.is_none()).map(|l| l.id).unwrap();
    d.w.dispatch(UiAction::Layer { action: LayerAction::Select { id: paint, mask: false } });
    pump(200);
    if d.w.notice.root.is_mapped() && state(&d.w).notice.is_some() {
        let rows = state(&d.w).layers.len();
        let label = notice_button(&d, NoticeActionId::NewPaintLayer).upcast::<gtk::Widget>();
        d.click(&label);
        pump(400);
        let stale = d.w.localization().text(layer_ui::MessageId::OBJECTS_NOTICE_STALE).to_string();
        if state(&d.w).layers.len() != rows { findings.push(format!("{theme} a stale notice action added a layer")); }
        let shown = (d.w.status.is_mapped() && d.w.status.text() == stale) || (d.w.notice.root.is_mapped() && state(&d.w).notice.is_some_and(|n| n.text == stale));
        if !shown { findings.push(format!("{theme} a stale notice action showed nothing; status {:?} (mapped {}), notice {:?}", d.w.status.text(), d.w.status.is_mapped(), state(&d.w).notice.map(|n| n.text))); }
        capture(&d, &format!("de-notice-stale-{theme}.png"));
    }
    d.w.dispatch(UiAction::Layer { action: LayerAction::Select { id: vector, mask: false } });
    wait_for("the vector layer for erasing", || active_vector(&d));

    d.w.dispatch(UiAction::Invoke { command: CommandId::Eraser });
    let eraser = open_drawer(&mut d, ToolbarControl::Command { command: CommandId::Eraser }, modes_shown);
    pump(400);
    findings.extend(mode_findings(&d, &format!("{theme} eraser drawer")));
    capture(&d, &format!("de-eraser-drawer-{theme}.png"));
    close_drawer(&mut d, &eraser);

    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(layer_ui::WorkspaceState::default()) });
    pump(600);
    d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(300);
    findings.extend(footer_findings(&d, &format!("{theme} docked")));
    findings.extend(mode_findings(&d, &format!("{theme} docked")));
    refuse_pencil(&mut d, 800.);
    findings.extend(notice_findings(&d, &format!("{theme} docked")));
    capture(&d, &format!("de-docked-{theme}.png"));
    d.w.dispatch(UiAction::Invoke { command: CommandId::Eraser });
    pump(300);
    findings.extend(mode_findings(&d, &format!("{theme} docked eraser")));

    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState { layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default() }) });
    pump(600);
    let layers = open_layers(&mut d);
    findings.extend(footer_findings(&d, &format!("{theme} drawer again")));
    close_drawer(&mut d, &layers);
    report(&findings);
    d.finish();
}

fn row(d: &Driver, id: u64) -> gtk::Widget {
    widgets(d.w.window.upcast_ref()).find(|w| w.is_mapped() && w.widget_name() == format!("art-layer-{id}")).unwrap_or_else(|| panic!("no mapped row for layer {id}"))
}
fn row_order(d: &Driver) -> Vec<u64> { state(&d.w).layers.iter().map(|l| l.id).collect() }
fn layer_paths(d: &Driver, id: u64) -> Vec<layer_core::authored::vector::PathItem> {
    let session = ui_session(&d.w);
    let doc = session.engine().document();
    occurrence_handle(id).ok().and_then(|h| doc.scene().vector_layer(h)).map_or_else(Vec::new, |o| o.paths().map(|(_, p)| p.clone()).collect())
}
fn menu_labels(menu: &layer_ui::ContextMenu) -> Vec<String> {
    fn visit(items: &[Vec<layer_ui::ContextMenuItem>], out: &mut Vec<String>) {
        for item in items.iter().flatten() { out.push(item.label.clone()); visit(&item.sections, out); }
    }
    let mut out = Vec::new();
    visit(&menu.sections, &mut out);
    out
}
fn menu_path(items: &[Vec<layer_ui::ContextMenuItem>], label: &str) -> Option<Vec<String>> {
    items.iter().flatten().find_map(|item| if item.label == label { Some(vec![item.label.clone()]) } else {
        menu_path(&item.sections, label).map(|rest| std::iter::once(item.label.clone()).chain(rest).collect())
    })
}
fn choose_menu_item(d: &mut Driver, popover: &gtk::PopoverMenu, menu: &layer_ui::ContextMenu, label: &str) {
    let path = menu_path(&menu.sections, label).unwrap_or_else(|| panic!("the menu offers no {label}"));
    for step in path {
        let find = || widgets(popover.upcast_ref()).find(|w| w.type_().name() == "GtkModelButton" && w.is_mapped() && w.width() > 0 && w.property::<String>("text") == step);
        wait_for(&format!("the menu item {step}"), || find().is_some());
        d.click(&find().unwrap());
        pump(300);
    }
}
fn context_menu(d: &mut Driver, id: u64) -> gtk::PopoverMenu {
    let target = row(d, id);
    let name = find_css(&target, "layer-name").unwrap_or(target);
    let p = d.point(&name);
    d.input.perform(serde_json::json!([{"point":p},{"button":273,"down":true},{"button":273,"down":false}]));
    wait_for("the row's context menu", || widgets(d.w.window.upcast_ref()).any(|w| w.is::<gtk::PopoverMenu>() && w.is_mapped()));
    pump(250);
    let menu=widgets(d.w.window.upcast_ref()).find_map(|w| w.downcast::<gtk::PopoverMenu>().ok().filter(|p| p.is_mapped())).unwrap();
    assert!(!menu.has_arrow());
    crate::menu_presentation::assert_declared(menu.upcast_ref());
    menu
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_layer_rows"]
fn native_vector_layer_rows() {
    let mut d = Driver::new("art.capycanvas.VectorRows");
    let theme = theme_name();
    let mut findings = Vec::new();
    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(layer_ui::WorkspaceState::default()) });
    pump(600);
    d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    d.w.dispatch(UiAction::SetColor { rgba: [0.1, 0.1, 0.12, 1.] });
    let footer = widgets(d.w.window.upcast_ref()).find(|w| w.widget_name() == "layer-add-vector-layer-symbolic" && w.is_mapped() && w.width() > 0).expect("the docked footer");
    d.click(&footer);
    wait_for("a vector layer", || active_vector(&d));
    d.w.dispatch(UiAction::SetBrushSize { value: 14. });
    pump(200);
    let [w, h] = ui_session(&d.w).engine().document().composition().size.map(|v| v as f32);
    let line = [[w * 0.3, h * 0.4], [w * 0.7, h * 0.4]];
    let probe = [w * 0.5, h * 0.4];
    draw(&mut d, "mouse", &line_points(line[0], line[1], 16));
    until_paths(&d, "a mouse line", |paths| paths.len() == 1);
    expect_ink(&mut d, &format!("{theme} the line"), probe, true);
    let vector = state(&d.w).layers.into_iter().find(|l| l.selected).unwrap().id;
    let painted = layer_paths(&d, vector);

    let popover = context_menu(&mut d, vector);
    let shared = menu_labels(&ui_session(&d.w).layer_menu(vector, false).unwrap());
    let shown: Vec<String> = widgets(popover.upcast_ref()).filter(|w| w.type_().name() == "GtkModelButton").map(|w| w.property::<String>("text")).collect();
    for label in &shared { if !shown.contains(label) { findings.push(format!("{theme} the vector row's menu lacks {label}")); } }
    let rasterize = CommandId::RasterizeLayer.localized_label(d.w.localization().as_ref()).to_string();
    if !shared.contains(&rasterize) { findings.push(format!("{theme} the vector row's menu offers no {rasterize}")); }
    capture(&d, &format!("rows-menu-{theme}.png"));
    let duplicate = d.w.localization().text(layer_ui::MessageId::RESOURCES_LAYER_MENU_DUPLICATE).to_string();
    let menu = ui_session(&d.w).layer_menu(vector, false).unwrap();
    choose_menu_item(&mut d, &popover, &menu, &duplicate);
    wait_for("Duplicate adds a second vector layer", || vector_layers(&d) == 2);
    let copy = state(&d.w).layers.into_iter().find(|l| l.selected).unwrap();
    assert_eq!(copy.content_icon.as_deref(), Some("layer-path-symbolic"), "the copy keeps the path badge");
    assert_eq!(layer_paths(&d, copy.id).len(), painted.len(), "the copy holds the paths");
    assert_eq!(layer_paths(&d, copy.id).iter().map(|p| p.geometry.clone()).collect::<Vec<_>>(), painted.iter().map(|p| p.geometry.clone()).collect::<Vec<_>>(), "the copy keeps the geometry");

    let name = find_css(&row(&d, copy.id), "layer-name").unwrap();
    let p = d.point(&name);
    d.input.perform(serde_json::json!([{"point":p},{"down":true},{"down":false},{"down":true},{"down":false}]));
    wait_for("double-clicking the name starts renaming", || state(&d.w).layer_tools.rename_layer == Some(copy.id));
    pump(200);
    chord(&mut d, &[CONTROL], 0x61);
    for key in "Ink".chars() { d.input.key(key as u32); }
    d.input.key(0xff0d);
    wait_for("the rename", || state(&d.w).layers.iter().any(|l| l.id == copy.id && l.label == "Ink"));
    capture(&d, &format!("rows-renamed-{theme}.png"));

    let eye = row(&d, copy.id).first_child().unwrap();
    d.click(&eye);
    wait_for("hiding the copy", || state(&d.w).layers.iter().any(|l| l.id == copy.id && !l.visible));
    expect_ink(&mut d, &format!("{theme} the original still shows"), probe, true);
    let eye = row(&d, vector).first_child().unwrap();
    d.click(&eye);
    wait_for("hiding the original", || state(&d.w).layers.iter().any(|l| l.id == vector && !l.visible));
    expect_ink(&mut d, &format!("{theme} both hidden"), probe, false);
    capture(&d, &format!("rows-hidden-{theme}.png"));
    for id in [vector, copy.id] { d.click(&row(&d, id).first_child().unwrap()); }
    wait_for("showing both again", || state(&d.w).layers.iter().filter(|l| l.content_icon.as_deref() == Some("layer-path-symbolic")).all(|l| l.visible));
    expect_ink(&mut d, &format!("{theme} shown again"), probe, true);

    d.w.dispatch(UiAction::Layer { action: LayerAction::Select { id: copy.id, mask: false } });
    pump(200);
    let lock = widgets(d.w.window.upcast_ref()).filter_map(|w| w.downcast::<gtk::ToggleButton>().ok())
        .find(|b| b.is_mapped() && find_named(b.upcast_ref(), "layer-lock-symbolic").is_some()).expect("the lock toggle");
    d.click(lock.upcast_ref());
    wait_for("locking the vector layer", || state(&d.w).layers.iter().any(|l| l.id == copy.id && l.locked));
    let count = layer_paths(&d, copy.id).len();
    draw(&mut d, "mouse", &line_points([w * 0.3, h * 0.6], [w * 0.7, h * 0.6], 12));
    wait_for("a locked vector layer refuses the pen", || state(&d.w).notice.is_some());
    assert_eq!(layer_paths(&d, copy.id).len(), count, "the locked layer keeps its paths");
    capture(&d, &format!("rows-locked-pen-{theme}.png"));
    d.w.dispatch(UiAction::Invoke { command: CommandId::Move });
    pump(200);
    draw(&mut d, "mouse", &line_points(probe, [probe[0], probe[1] + h * 0.1], 6));
    pump(300);
    if layer_paths(&d, copy.id).iter().map(|p| p.geometry.clone()).collect::<Vec<_>>() != painted.iter().map(|p| p.geometry.clone()).collect::<Vec<_>>() { findings.push(format!("{theme} Move dragged a path on a locked vector layer")); }
    d.click(lock.upcast_ref());
    wait_for("unlocking", || state(&d.w).layers.iter().any(|l| l.id == copy.id && !l.locked));
    d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 });

    let before = row_order(&d);
    let below = before.iter().position(|id| *id == vector).unwrap();
    let source = find_css(&row(&d, copy.id), "layer-name").unwrap();
    let start = d.point(&source);
    let target = row(&d, before[below + 1]).compute_bounds(&d.w.window).unwrap();
    let drop = [target.x() + target.width() / 2., target.y() + target.height() - 3.];
    let slop = gtk::Settings::default().unwrap().gtk_dnd_drag_threshold() as f32;
    d.input.perform(serde_json::json!([{"point":start},{"down":true},{"point":[start[0], start[1] + slop * 3.]},{"point":[drop[0], drop[1] - 10.]},{"point":drop},{"down":false}]));
    wait_for("dragging the vector row below the next row", || row_order(&d) != before);
    let after = row_order(&d);
    assert!(after.iter().position(|id| *id == copy.id).unwrap() > after.iter().position(|id| *id == vector).unwrap(), "the copy moved below the original: {before:?} -> {after:?}");
    assert_eq!(layer_paths(&d, copy.id).len(), painted.len(), "reordering keeps the paths");
    capture(&d, &format!("rows-reordered-{theme}.png"));
    chord(&mut d, &[CONTROL], 0x7a);
    wait_for("Ctrl+Z restores the row order", || row_order(&d) == before);

    d.w.dispatch(UiAction::Layer { action: LayerAction::Select { id: copy.id, mask: false } });
    pump(200);
    let delete = widgets(d.w.window.upcast_ref()).find(|w| w.widget_name() == "delete-selected-layers" && w.is_mapped() && w.width() > 0).expect("the footer's delete");
    d.click(&delete);
    wait_for("deleting the copy", || vector_layers(&d) == 1 && !state(&d.w).layers.iter().any(|l| l.id == copy.id));
    expect_ink(&mut d, &format!("{theme} the original survives the delete"), probe, true);
    chord(&mut d, &[CONTROL], 0x7a);
    wait_for("Ctrl+Z brings the deleted vector layer back", || vector_layers(&d) == 2);
    let restored = state(&d.w).layers.into_iter().find(|l| l.label == "Ink").expect("the copy returns with its name");
    assert_eq!(layer_paths(&d, restored.id).len(), painted.len(), "the restored copy keeps its paths");
    capture(&d, &format!("rows-restored-{theme}.png"));
    report(&findings);
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_save_reopen_and_resize --native-storage"]
#[allow(deprecated)]
fn native_vector_save_reopen_and_resize() {
    use layer_ui::VectorEraser;
    let mut d = Driver::new("art.capycanvas.VectorSave");
    let theme = theme_name();
    d.w.dispatch(UiAction::SetColor { rgba: [0.85, 0.35, 0.1, 1.] });
    add_vector_layer(&mut d);
    d.w.dispatch(UiAction::SetBrushSize { value: 14. });
    pump(200);
    draw(&mut d, "mouse", &line_points([400., 300.], [1200., 340.], 20));
    until_paths(&d, "a G-Pen line", |paths| paths.len() == 1);
    tool(&d, LayerCanvasTool::Figure { shape: FigureShape::Rectangle, paint: FigurePaint::Both });
    draw(&mut d, "mouse", &line_points([400., 500.], [800., 760.], 6));
    until_paths(&d, "a live rectangle", |paths| paths.len() == 2 && paths.iter().any(|p| p.shape.is_some()));
    tool(&d, LayerCanvasTool::Figure { shape: FigureShape::Star, paint: FigurePaint::Outline });
    draw(&mut d, "mouse", &line_points([1000., 500.], [1300., 800.], 6));
    until_paths(&d, "a live star", |paths| paths.len() == 3);
    let layer = occurrence_token(ui_session(&d.w).engine().document().working.occurrence.unwrap());
    let blue = layer_core::color::RgbColor::new(Default::default(), [0.1, 0.3, 0.9, 1.]).unwrap();
    d.w.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "path_outline_color".into(), value: layer_core::EffectValue::Color(blue) } });
    d.w.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "path_width".into(), value: layer_core::EffectValue::Number(12.) } });
    until_paths(&d, "restyling the star", |paths| paths.iter().any(|p| p.style.strokes().last().is_some_and(|s| s.width == 12.)));
    let saved = paths(&d);
    let name = state(&d.w).layers.into_iter().find(|l| l.selected).unwrap().label;

    d.w.dispatch(UiAction::Invoke { command: CommandId::Eraser });
    let eraser = open_drawer(&mut d, ToolbarControl::Command { command: CommandId::Eraser }, modes_shown);
    let whole = CommandId::EraseWholePaths.localized_label(d.w.localization().as_ref()).to_string();
    d.click_label(&whole);
    wait_for("Erase whole paths", || state(&d.w).settings.vector_eraser == VectorEraser::WholePaths);
    close_drawer(&mut d, &eraser);

    d.w.dispatch(UiAction::Invoke { command: CommandId::Move });
    pump(200);
    d.input.key(0xff1b);
    wait_for("Escape clears the paths before the drag", || selected_items(&d).is_empty());
    let star = saved.iter().find(|p| p.shape.is_some() && start(p)[0] > 900.).unwrap().clone();
    let grab = start(&star);
    let hit = grab;
    let history = checkpoint(&d);
    let a = canvas_point(&d, grab);
    d.input.perform(serde_json::json!([{"point":a},{"down":true},{"point":[a[0] + 10., a[1] + 10.]},{"point":[a[0] + 40., a[1] + 30.]}]));
    capture(&d, &format!("save-drag-before-resize-{theme}.png"));
    d.w.window.unmaximize();
    d.w.window.set_default_size(1100, 760);
    wait_for("the window shrinks mid-drag", || d.w.window.width() < 1200);
    pump(400);
    capture(&d, &format!("save-drag-shrunk-{theme}.png"));
    d.w.window.maximize();
    wait_for("the window grows back mid-drag", || d.w.window.width() > 1500);
    pump(500);
    capture(&d, &format!("save-drag-regrown-{theme}.png"));
    let target = [grab[0] + 120., grab[1] + 90.];
    let b = canvas_point(&d, target);
    d.input.perform(serde_json::json!([{"point":[b[0] - 20., b[1] - 10.]},{"point":b},{"down":false}]));
    let moved = until_paths(&d, "the drag across the resize commits", |paths| paths.iter().any(|p| p.id == star.id && p.geometry != star.geometry));
    let dropped = moved.iter().find(|p| p.id == star.id).unwrap();
    let offset = [start(dropped)[0] - start(&star)[0], start(dropped)[1] - start(&star)[1]];
    assert!((offset[0] - 120.).abs() < 4. && (offset[1] - 90.).abs() < 4., "the dragged star follows the pointer across the resize: {offset:?}");
    assert!(dropped.shape.is_some(), "the moved star stays a live shape");
    assert_ne!(checkpoint(&d), history, "the drag is one undo step");
    expect_ink(&mut d, &format!("{theme} no drag preview left at the old place"), [hit[0], hit[1]], false);
    capture(&d, &format!("save-resized-drag-{theme}.png"));
    chord(&mut d, &[CONTROL], 0x7a);
    until_paths(&d, "undoing the drag", |paths| paths.iter().any(|p| p.id == star.id && p.geometry == star.geometry));

    let directory = d.input.dir.clone();
    new_photo::invoke(&d.w, CommandId::SaveDocumentAs);
    let save = new_photo::chooser();
    save.set_current_folder(Some(&gtk::gio::File::for_path(&directory))).unwrap();
    save.set_current_name("vector-edges.capy");
    pump(100);
    save.response(gtk::ResponseType::Accept);
    new_photo::finish(&d.w);
    assert!(!state(&d.w).document_file.modified, "{:?}", state(&d.w).host_error);
    let path = directory.join("vector-edges.capy");
    assert!(path.is_file());
    let tab = d.w.documents.selected();
    new_photo::invoke(&d.w, CommandId::OpenDocument);
    let open = new_photo::chooser();
    open.set_file(&gtk::gio::File::for_path(&path)).unwrap();
    pump(100);
    open.response(gtk::ResponseType::Accept);
    new_photo::finish(&d.w);
    until(|| d.w.documents.selected() != tab, "the saved drawing reopens in its own tab");
    new_photo::ready(&d.w);
    let reopened = state(&d.w).layers.into_iter().find(|l| l.label == name).expect("the vector layer reopens");
    assert_eq!(reopened.content_icon.as_deref(), Some("layer-path-symbolic"));
    d.w.dispatch(UiAction::Layer { action: LayerAction::Select { id: reopened.id, mask: false } });
    pump(300);
    assert_eq!(paths(&d), saved, "paths, styles and live shapes survive saving");
    expect_ink(&mut d, &format!("{theme} the reopened line"), [800., 320.], true);
    capture(&d, &format!("save-reopened-{theme}.png"));

    let settings = std::fs::read_to_string(crate::storage::roots().unwrap().settings()).unwrap();
    assert!(settings.contains("\"vector_eraser\": \"whole_paths\"") || settings.contains("\"vector_eraser\":\"whole_paths\""), "the eraser mode is saved: {settings}");
    d.w.window.close();
    until(|| !d.w.window.is_visible(), "window close");
    d.w = Workspace::new(&d._app);
    d.w.window.maximize();
    d.w.window.present();
    wait_workspaces(&d.w);
    pump(800);
    assert_eq!(state(&d.w).settings.vector_eraser, VectorEraser::WholePaths, "the eraser mode survives a restart");
    d.finish();
}

fn use_preset(d: &mut Driver, preset: layer_ui::WorkspacePreset) {
    let wanted = preset.layout(layer_ui::Platform::Gtk);
    for _ in 0..4 {
        d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(layer_ui::WorkspaceState { layout: wanted.clone(), ..Default::default() }) });
        pump(800);
        if state(&d.w).workspace.layout.header == wanted.header { break; }
    }
    assert_eq!(state(&d.w).workspace.layout.header, wanted.header, "the {preset:?} layout");
    d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(300);
}
fn label_shown(d: &Driver, text: &str) -> bool {
    widgets(d.w.window.upcast_ref()).any(|w| w.is_mapped() && w.width() > 0 && w.downcast_ref::<gtk::Label>().is_some_and(|l| l.text() == text))
}
fn notice_offers(d: &Driver, action: NoticeActionId) -> bool {
    state(&d.w).notice.as_ref().is_some_and(|n| n.actions.iter().any(|a| a.id == action)) && d.w.notice.root.is_mapped()
}
fn live_capture(d: &mut Driver, name: &str) {
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() { d.input.perform(serde_json::json!([{"capture": name}])); }
}
fn scale_rotate_selected(d: &Driver) -> bool { ui_session(&d.w).command(CommandId::ScaleRotate).selected }
fn tooltip_shown(d: &Driver) -> Option<String> {
    widgets(d.w.window.upcast_ref()).find(|w| w.type_().name() == "GtkTooltipWindow" && w.is_mapped())
        .map(|tip| widgets(&tip).filter_map(|w| w.downcast::<gtk::Label>().ok()).map(|l| l.text().to_string()).collect::<Vec<_>>().join(" "))
}
fn notice_shape_findings(d: &Driver, place: &str) -> Vec<String> {
    let root: gtk::Widget = d.w.notice.root.clone().upcast();
    let mut found = Vec::new();
    let Some(text) = widgets(&root).find_map(|w| w.downcast::<gtk::Label>().ok().filter(|l| l.has_css_class("canvas-action-bar-label"))) else { return vec![format!("{place}: the notice has no caption")]; };
    let lines = text.layout().line_count();
    let beside = d.w.notice.root.orientation() == gtk::Orientation::Horizontal;
    if beside && lines > 1 { found.push(format!("{place}: the text wraps to {lines} lines beside the actions")); }
    if beside && root.height() != 36 { found.push(format!("{place}: a one-row notice is {} px tall, not the bar's 36", root.height())); }
    let actions: Vec<gtk::Widget> = widgets(&root).filter(|w| w.widget_name() == "canvas-notice-action" && w.is_mapped()).collect();
    let heights: Vec<i32> = actions.iter().map(|w| w.height()).collect();
    if heights.iter().any(|h| *h != 26) { found.push(format!("{place}: action heights {heights:?} differ from the 26 px worded bar control")); }
    if !beside && let (Some(row), Some(caption)) = (find_named(&root, "canvas-notice-actions"), text.compute_bounds(&root)) && let Some(bounds) = row.compute_bounds(&root) {
        if bounds.y() < caption.y() + caption.height() - 1. { found.push(format!("{place}: stacked actions overlap the text")); }
        if (root.width() as f32 - bounds.x() - bounds.width()).abs() > 1. { found.push(format!("{place}: stacked actions end {} px from the right edge", root.width() as f32 - bounds.x() - bounds.width())); }
    }
    found
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_sketch_paths_and_explanations --tablet"]
fn native_vector_sketch_paths_and_explanations() {
    let mut d = Driver::new("art.capycanvas.VectorSketchPaths");
    let theme = theme_name();
    let mut findings = Vec::new();
    pump(2000);
    use_preset(&mut d, layer_ui::WorkspacePreset::Painter);
    d.w.dispatch(UiAction::SetColor { rgba: [0.1, 0.1, 0.12, 1.] });
    add_vector_layer(&mut d);
    d.w.dispatch(UiAction::SetBrushSize { value: 14. });
    pump(100);
    super::vector_layers::pen_points(&mut d, &super::vector_layers::stroke([400., 300.], [900., 340.]));
    until_paths(&d, "a line to pick", |paths| paths.len() == 1);

    let unavailable = d.w.localization().text(layer_ui::MessageId::OBJECTS_VECTOR_UNAVAILABLE).to_string();
    let rough = format!("brush-{}", DefaultBrushPreset::RoughGPen as u32);
    let drawer = open_drawer(&mut d, ToolbarControl::Command { command: CommandId::DrawingBrush }, |d| find_named(d.w.window.upcast_ref(), &rough).is_some_and(|w| w.is_mapped()));
    pump(300);
    let row = d.named(&rough);
    if !row.is_sensitive() || !row.has_css_class("unavailable-tool") || row.tooltip_text().as_deref() != Some(unavailable.as_str()) {
        findings.push(format!("{theme}: dimmed Rough G-Pen: sensitive {}, dimmed {}, tooltip {:?}", row.is_sensitive(), row.has_css_class("unavailable-tool"), row.tooltip_text()));
    }
    let hover = d.point(&row);
    d.input.perform(serde_json::json!([{"point": [hover[0] - 20., hover[1]]}, {"point": hover}]));
    let deadline = Instant::now() + Duration::from_secs(3);
    while tooltip_shown(&d).is_none() && Instant::now() < deadline { pump(50); }
    if !tooltip_shown(&d).is_some_and(|text| text.contains(&unavailable)) { findings.push(format!("{theme}: hovering dimmed Rough G-Pen shows {:?}", tooltip_shown(&d))); }
    live_capture(&mut d, &format!("sketch-dimmed-tooltip-{theme}"));
    d.click_name(&rough);
    wait_for("choosing the dimmed pen explains at once", || state(&d.w).brush.preset == DefaultBrushPreset::RoughGPen as u32 && notice_offers(&d, NoticeActionId::UseVectorPen));
    findings.extend(notice_shape_findings(&d, &format!("{theme} dimmed Rough G-Pen")));
    live_capture(&mut d, &format!("sketch-dimmed-chosen-{theme}"));
    press_notice(&mut d, NoticeActionId::UseVectorPen);
    wait_for("Use G-Pen from the dimmed pen's notice", || state(&d.w).brush.preset == DefaultBrushPreset::GPen as u32);
    close_drawer(&mut d, &drawer);

    d.click_name(&tool_opener(&d, ToolbarControl::Command { command: CommandId::ScaleRotate }));
    wait_for("Scale/rotate picks paths", || canvas_tool(&d) == LayerCanvasTool::Move && scale_rotate_selected(&d));
    super::vector_layers::pen_points(&mut d, &[[650., 320.], [650., 320.]]);
    wait_for("a tap selects the line", || !selected_items(&d).is_empty());
    capture(&d, &format!("sketch-scale-rotate-picks-{theme}.png"));
    d.w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    pump(200);

    let localization = d.w.localization();
    let select_paths = localization.text(layer_ui::MessageId::TOOL_SELECT_PATHS).to_string();
    let edit_points = CommandId::EditPoints.localized_label(localization.as_ref()).to_string();
    let drawer = open_drawer(&mut d, ToolbarControl::Command { command: CommandId::Select }, |d| label_shown(d, &select_paths));
    pump(300);
    if !label_shown(&d, &edit_points) { findings.push(format!("{theme}: the Select drawer does not show {edit_points}")); }
    capture(&d, &format!("sketch-select-drawer-paths-{theme}.png"));
    d.click_label(&select_paths);
    wait_for("Select paths picks the path tool", || canvas_tool(&d) == LayerCanvasTool::Move && scale_rotate_selected(&d));
    close_drawer(&mut d, &drawer);
    super::vector_layers::pen_points(&mut d, &[[650., 320.], [650., 320.]]);
    wait_for("the path tool selects the line", || !selected_items(&d).is_empty());
    d.w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    pump(200);

    d.click_name(&tool_opener(&d, ToolbarControl::Command { command: CommandId::Sculpt }));
    wait_for("Sculpt explains at once", || notice_offers(&d, NoticeActionId::UseVectorPen));
    findings.extend(notice_findings(&d, &format!("{theme} Sculpt")));
    findings.extend(notice_shape_findings(&d, &format!("{theme} Sculpt")));
    live_capture(&mut d, &format!("sketch-sculpt-notice-{theme}"));
    capture(&d, &format!("sketch-sculpt-explains-{theme}.png"));
    press_notice(&mut d, NoticeActionId::UseVectorPen);
    wait_for("Use G-Pen", || state(&d.w).brush.preset == DefaultBrushPreset::GPen as u32 && state(&d.w).notice.is_none());

    let vector = state(&d.w).layers.iter().find(|l| l.selected).unwrap().id;
    d.w.dispatch(UiAction::Layer { action: LayerAction::Lock { id: vector, value: true } });
    pump(300);
    let locked = d.w.localization().text(layer_ui::MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string();
    let header = d.named(&tool_opener(&d, ToolbarControl::Command { command: CommandId::ScaleRotate }));
    let scale_rotate = header.clone().downcast::<gtk::Button>().ok().or_else(|| descendant::<gtk::Button>(&header)).unwrap();
    if scale_rotate.tooltip_text().as_deref() != Some(locked.as_str()) { findings.push(format!("{theme}: greyed Scale/rotate says {:?}", scale_rotate.tooltip_text())); }
    d.w.dispatch(UiAction::Layer { action: LayerAction::Lock { id: vector, value: false } });
    pump(300);

    use_preset(&mut d, layer_ui::WorkspacePreset::Illustrator);
    d.click_name(&tool_opener(&d, ToolbarControl::Command { command: CommandId::Brush }));
    wait_for("the Brush tile explains at once", || notice_offers(&d, NoticeActionId::UseVectorPen));
    findings.extend(notice_findings(&d, &format!("{theme} Paint Brush")));
    findings.extend(notice_shape_findings(&d, &format!("{theme} Paint Brush")));
    live_capture(&mut d, &format!("paint-brush-notice-{theme}"));
    capture(&d, &format!("paint-brush-explains-{theme}.png"));
    d.click_name(&tool_opener(&d, ToolbarControl::Command { command: CommandId::Pen }));
    wait_for("a pen that draws paths clears it", || state(&d.w).notice.is_none() && state(&d.w).brush.preset == DefaultBrushPreset::GPen as u32);
    report(&findings);
    d.finish();
}
