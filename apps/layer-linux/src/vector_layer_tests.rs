//! GTK vector layers: G-Pen lines with actual compositor contacts, then
//! selecting, dragging, undoing and deleting paths with the Move tool.
use super::*;
use super::selection_tools::{canvas_drag, canvas_point};
use layer_core::authored::vector::PathItem;

pub(super) fn paths(d: &Driver) -> Vec<PathItem> {
    let session = ui_session(&d.w);
    let doc = session.engine().document();
    doc.working.occurrence.and_then(|layer| doc.scene().vector_layer(layer)).map_or_else(Vec::new, |object| object.paths().map(|(_, p)| p.clone()).collect())
}
pub(super) fn until_paths(d: &Driver, what: &str, done: impl Fn(&[PathItem]) -> bool) -> Vec<PathItem> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        pump(20);
        let paths = paths(d);
        if done(&paths) { return paths; }
        assert!(Instant::now() < deadline, "{what} timed out: {}", d.w.status.text());
    }
}
pub(super) fn start(path: &PathItem) -> [f32; 2] { path.geometry.subpaths()[0].anchor(0).point().map(|v| v as f32) }

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_draw_move_and_delete --tablet"]
fn native_vector_draw_move_and_delete() {
    let mut d = Driver::new("art.capycanvas.VectorMove");
    d.w.dispatch(UiAction::Layer { action: LayerAction::NewVector });
    d.w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::GPen as u32 });
    pump(300);
    for row in 0..2 {
        let points: Vec<[f32; 2]> = (0..48).map(|i| canvas_point(&d, [500. + i as f32 * 14., 600. + row as f32 * 260. + (i as f32 * 0.25).sin() * 90.])).collect();
        let mut script = vec![serde_json::json!({"pen":"move","point":points[0]}), serde_json::json!({"pen":"down","point":points[0]})];
        script.extend(points[1..].iter().map(|p| serde_json::json!({"pen":"move","point":p})));
        script.extend([serde_json::json!({"pen":"up"}), serde_json::json!({"pen":"leave"})]);
        d.input.perform(serde_json::Value::Array(script));
        until_paths(&d, "a fitted G-Pen line", |paths| paths.len() == row + 1);
    }
    d.capture_canvas("vector-lines.png");
    d.w.dispatch(UiAction::Invoke { command: CommandId::Move });
    pump(200);
    let before = paths(&d);
    let front = before[0].clone();
    let [x, y] = start(&front);
    canvas_drag(&mut d, [x, y], [x + 150., y + 90.], true);
    let moved = until_paths(&d, "the dragged path", |paths| paths.iter().any(|p| p.id == front.id && (start(p)[0] - x - 150.).abs() < 2. && (start(p)[1] - y - 90.).abs() < 2.));
    assert!(moved.iter().any(|p| p.id == before[1].id && start(p) == start(&before[1])), "the other line stays");
    assert!(ui_session(&d.w).engine().document().working.vector_items.as_ref().is_some_and(|s| s.items.contains(&front.id)));
    d.capture_canvas("vector-moved.png");
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until_paths(&d, "undoing the move", |paths| paths.iter().any(|p| p.id == front.id && start(p) == [x, y]));
    let [x0, y0, x1, y1] = before[1].bounds().map(|v| v as f32);
    canvas_drag(&mut d, [x0 - 20., y0 - 20.], [x1 + 20., y1 + 20.], true);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ui_session(&d.w).engine().document().working.vector_items.as_ref().is_some_and(|s| s.items.contains(&before[1].id)) {
        pump(20);
        assert!(Instant::now() < deadline, "marquee selection timed out");
    }
    d.capture_canvas("vector-marquee.png");
    d.w.dispatch(UiAction::Invoke { command: CommandId::ClearSelected });
    until_paths(&d, "deleting the selected line", |paths| paths.iter().all(|p| p.id != before[1].id));
    d.capture_canvas("vector-deleted.png");
    d.finish();
}

pub(super) fn pen_points(d: &mut Driver, points: &[[f32; 2]]) {
    let points: Vec<[f32; 2]> = points.iter().map(|p| canvas_point(d, *p)).collect();
    let mut script = vec![serde_json::json!({"pen":"move","point":points[0]}), serde_json::json!({"pen":"down","point":points[0]})];
    script.extend(points[1..].iter().map(|p| serde_json::json!({"pen":"move","point":p})));
    script.extend([serde_json::json!({"pen":"up"}), serde_json::json!({"pen":"leave"})]);
    d.input.perform(serde_json::Value::Array(script));
}
pub(super) fn tool(d: &Driver, tool: LayerCanvasTool) {
    d.w.dispatch(UiAction::Layer { action: LayerAction::Tool { tool } });
    pump(200);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_tools --tablet"]
fn native_vector_tools() {
    use layer_core::{FigurePaint, FigureShape};
    let mut d = Driver::new("art.capycanvas.VectorTools");
    d.w.dispatch(UiAction::Layer { action: LayerAction::NewVector });
    d.w.dispatch(UiAction::SetColor { rgba: [0.85, 0.35, 0.1, 1.] });
    pump(300);
    tool(&d, LayerCanvasTool::Figure { shape: FigureShape::Rectangle, paint: FigurePaint::Both });
    pen_points(&mut d, &[[400., 400.], [600., 520.], [800., 640.]]);
    let rectangle = until_paths(&d, "a Figure rectangle", |paths| paths.len() == 1)[0].clone();
    assert!(rectangle.shape.is_some(), "the rectangle keeps its live shape");
    d.capture_canvas("vector-figure.png");

    tool(&d, LayerCanvasTool::Figure { shape: FigureShape::Path, paint: FigurePaint::Outline });
    for at in [[900., 400.], [1000., 650.], [1100., 420.]] { pen_points(&mut d, &[at, at]); pump(80); }
    pen_points(&mut d, &[[1200., 640.], [1250., 700.]]);
    pump(200);
    d.capture_canvas("vector-path-drafting.png");
    d.input.key(0xff0d);
    let drawn = until_paths(&d, "a finished path", |paths| paths.len() == 2);
    assert_eq!(drawn[0].geometry.anchor_count(), 4);
    d.capture_canvas("vector-path.png");

    d.w.dispatch(UiAction::Invoke { command: CommandId::EditPoints });
    pump(200);
    let first = start(&drawn[0]);
    canvas_drag(&mut d, first, [first[0] - 60., first[1] + 120.], true);
    until_paths(&d, "an anchor drag", |paths| paths.iter().any(|p| p.id == drawn[0].id && (start(p)[0] - first[0] + 60.).abs() < 2.));
    d.capture_canvas("vector-edit-points.png");

    let layer = occurrence_token(ui_session(&d.w).engine().document().working.occurrence.unwrap());
    let blue = layer_core::color::RgbColor::new(Default::default(), [0.1, 0.3, 0.9, 1.]).unwrap();
    d.w.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "path_outline_color".into(), value: layer_core::EffectValue::Color(blue) } });
    d.w.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: "path_width".into(), value: layer_core::EffectValue::Number(12.) } });
    until_paths(&d, "restyling the path", |paths| paths.iter().any(|p| p.id == drawn[0].id && p.style.strokes().last().is_some_and(|s| s.width == 12.)));
    d.capture_canvas("vector-properties.png");

    d.w.dispatch(UiAction::Invoke { command: CommandId::Move });
    pump(200);
    let [x0, y0, x1, y1] = rectangle.visual_bounds().map(|v| v as f32);
    canvas_drag(&mut d, [x0 - 30., y0 - 30.], [x1 + 30., y1 + 30.], true);
    pump(300);
    d.capture_canvas("vector-frame.png");
    canvas_drag(&mut d, [x1, y1], [x1 + 120., y1 + 80.], true);
    until_paths(&d, "scaling with the frame", |paths| paths.iter().any(|p| p.id == rectangle.id && p.visual_bounds()[2] > f64::from(x1) + 100.));
    d.capture_canvas("vector-scaled.png");

    d.w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::GPen as u32 });
    pump(200);
    pen_points(&mut d, &(0..40).map(|i| [300. + i as f32 * 25., 900. + (i as f32 * 0.3).sin() * 40.]).collect::<Vec<_>>());
    let inked = until_paths(&d, "an inked line", |paths| paths.len() == 3);
    wait_for("the line crosses the eraser's center", || marked(presented(&d, [805., 890.])));
    d.w.dispatch(UiAction::Invoke { command: CommandId::Eraser });
    d.w.dispatch(UiAction::SetBrushSize { value: 8. });
    pump(200);
    pen_points(&mut d, &[[800., 820.], [805., 900.], [810., 980.]]);
    until_paths(&d, "erasing across the line", |paths| paths.len() == 4 && paths.iter().any(|p| p.id == inked[0].id));
    wait_for("the eraser clears its center and keeps both sides", || !marked(presented(&d, [805., 890.])) && marked(presented(&d, [700., 860.])) && marked(presented(&d, [1000., 930.])));
    d.capture_canvas("vector-erased.png");
    d.finish();
}

pub(super) fn presented(d: &Driver, document: [f32; 2]) -> [u8; 4] {
    let capture = ui_session(&d.w).engine().backend().capture_in(d.w.view_color()).unwrap();
    let m = state(&d.w).camera.document_to_surface();
    let x = (m[0] * document[0] + m[2] * document[1] + m[4]).round() as usize;
    let y = (m[1] * document[0] + m[3] * document[1] + m[5]).round() as usize;
    capture.bytes[y * capture.stride as usize + x * 4..][..4].try_into().unwrap()
}
pub(super) fn capture(d: &Driver, name: &str) {
    let _warm = crate::snapshot(&d.w);
    pump(120);
    crate::snapshot(&d.w).save_to_png(d.input.dir.join(name)).unwrap();
}
fn gesture_point(d:&Driver,point:[f32;2])->[f32;2] {
    let point=d.w.area.compute_point(&d.w.window,&gtk::graphene::Point::new(point[0],point[1])).unwrap();
    [point.x(),point.y()]
}
pub(super) fn inked(pixel: [u8; 4]) -> bool { pixel[..3].iter().all(|v| *v < 100) }
pub(super) fn marked(pixel: [u8; 4]) -> bool { pixel[..3].iter().map(|v| u16::from(*v)).sum::<u16>() < 600 }
fn greenish(pixel: [u8; 4]) -> bool { i16::from(pixel[1]) - i16::from(pixel[0]).max(i16::from(pixel[2])) > 40 }
pub(super) fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        pump(20);
        assert!(Instant::now() < deadline, "{what} timed out");
    }
}
pub(super) fn active_vector(d: &Driver) -> bool {
    let session = ui_session(&d.w);
    let doc = session.engine().document();
    doc.working.occurrence.is_some_and(|h| doc.scene().vector_layer(h).is_some())
}
pub(super) fn tool_opener(d: &Driver, control: ToolbarControl) -> String {
    let workspace = state(&d.w).workspace;
    let header = workspace.layout.header.entries().filter(|entry| entry.item == HeaderItem::Tool { control }).map(|entry| format!("header-item-{}", entry.id));
    let tiles = workspace.layout.panels.iter().flat_map(|panel| panel.tiles()).filter(|tile| tile.control == control).map(|tile| format!("tile-{}", tile.id));
    let names: Vec<_> = header.chain(tiles).collect();
    names.iter().find(|name| find_named(d.w.window.upcast_ref(), name).is_some_and(|widget| widget.is_mapped())).cloned().or_else(|| names.into_iter().next()).expect("a tool opener in the workspace")
}
pub(super) fn add_vector_layer(d: &mut Driver) {
    let shown = |d: &Driver| widgets(d.w.window.upcast_ref()).any(|widget| widget.widget_name() == "layer-add-vector-layer-symbolic" && widget.is_mapped());
    let opener = (!shown(d)).then(|| tool_opener(d, ToolbarControl::Panel { panel: Panel::Layers }));
    if let Some(opener) = &opener { d.click_name(opener); pump(150); }
    if state(&d.w).customization.drawer.as_ref().is_some_and(|drawer| drawer.layer_properties == Some(true)) { d.click_name("layer-properties-disclosure"); pump(150); }
    let footer = widgets(d.w.window.upcast_ref()).find(|widget| widget.widget_name() == "layer-add-vector-layer-symbolic" && widget.is_mapped()).expect("the visible New vector layer footer button");
    d.click(&footer);
    wait_for("a vector layer from the footer button", || active_vector(d));
    d.capture_canvas("footer-new-vector-layer.png");
    if let Some(opener) = opener && state(&d.w).customization.drawer.is_some() { d.click_name(&opener); }
    pump(150);
}
pub(super) fn press_notice(d: &mut Driver, action: layer_ui::NoticeActionId) {
    wait_for("the notice", || state(&d.w).notice.as_ref().is_some_and(|n| n.actions.iter().any(|a| a.id == action)));
    let label = state(&d.w).notice.unwrap().actions.into_iter().find(|a| a.id == action).unwrap().label;
    d.click_label(&label);
}
pub(super) fn stroke(from: [f32; 2], to: [f32; 2]) -> Vec<[f32; 2]> {
    (0..=24).map(|i| { let u = i as f32 / 24.; [from[0] + (to[0] - from[0]) * u, from[1] + (to[1] - from[1]) * u] }).collect()
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_discover_and_wrong_brush --tablet"]
fn native_vector_discover_and_wrong_brush() {
    use layer_core::DefaultBrushPreset;
    let mut d = Driver::new("art.capycanvas.VectorDiscover");
    d.w.dispatch(UiAction::SetColor { rgba: [0.1, 0.1, 0.12, 1.] });
    d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Pencil as u32 });
    pump(200);
    add_vector_layer(&mut d);
    assert_eq!(state(&d.w).brush.preset, DefaultBrushPreset::GPen as u32, "a pen that draws paths is chosen");
    let row = state(&d.w).layers.into_iter().find(|row| row.selected).unwrap();
    assert_eq!(row.content_icon.as_deref(), Some("layer-path-symbolic"));
    d.w.dispatch(UiAction::SetBrushSize { value: 14. });
    pump(100);
    pen_points(&mut d, &stroke([400., 300.], [900., 340.]));
    until_paths(&d, "the first inked line", |paths| paths.len() == 1);
    wait_for("the line on screen", || inked(presented(&d, [650., 320.])));
    d.capture_canvas("discover-ink.png");

    d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Pencil as u32 });
    pump(200);
    pen_points(&mut d, &stroke([400., 500.], [900., 540.]));
    wait_for("the vector refusal", || state(&d.w).notice.is_some());
    assert!(paths(&d).len() == 1, "the pencil leaves the paths alone");
    d.capture_canvas("wrong-brush-notice.png");
    press_notice(&mut d, layer_ui::NoticeActionId::UseVectorPen);
    wait_for("Use G-Pen", || state(&d.w).brush.preset == DefaultBrushPreset::GPen as u32);
    pen_points(&mut d, &stroke([400., 500.], [900., 540.]));
    until_paths(&d, "a line with the chosen pen", |paths| paths.len() == 2);

    d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Pencil as u32 });
    pump(200);
    pen_points(&mut d, &stroke([400., 700.], [900., 740.]));
    press_notice(&mut d, layer_ui::NoticeActionId::NewPaintLayer);
    wait_for("a paint layer above", || !active_vector(&d));
    assert_eq!(state(&d.w).brush.preset, DefaultBrushPreset::Pencil as u32, "the brush stays");
    d.w.dispatch(UiAction::SetBrushSize { value: 30. });
    pump(100);
    pen_points(&mut d, &stroke([400., 700.], [900., 740.]));
    wait_for("pencil pixels on the new layer", || marked(presented(&d, [650., 720.])));
    d.capture_canvas("wrong-brush-paint-layer.png");
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_fill_between_open_strokes --tablet"]
fn native_vector_fill_between_open_strokes() {
    let mut d=Driver::new("art.capycanvas.VectorFaces");
    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        add_vector_layer(&mut d);
        let target=ui_session(&d.w).engine().document().working.occurrence.unwrap();
        d.w.dispatch(UiAction::SelectBrush {id:layer_core::DefaultBrushPreset::GPen as u32});
        d.w.dispatch(UiAction::SetBrushSize {value:6.});
        d.w.dispatch(UiAction::SetColor {rgba:[0.,0.,0.,1.]});
        tool(&d,LayerCanvasTool::Paint);
        for (index,(from,to)) in [([350.,400.],[950.,400.]),([900.,350.],[900.,900.]),([950.,850.],[350.,850.]),([400.,900.],[400.,350.])].into_iter().enumerate() {
            pen_points(&mut d,&stroke(from,to));
            until_paths(&d,"a separate open bounding stroke",|paths|paths.len()==index+1);
        }
        let boundary=[[650.,400.],[900.,625.],[650.,850.],[400.,625.]];
        wait_for("the four bounding strokes on screen",||boundary.into_iter().all(|point|inked(presented(&d,point))));
        let boundary_pixels=boundary.map(|point|presented(&d,point));
        let original=paths(&d);
        assert!(original.iter().all(|path|path.style.strokes().next().is_some()&&!path.encloses([650.,625.],0.05)));
        let layers=ui_session(&d.w).engine().document().scene().order().to_vec();
        d.w.dispatch(UiAction::SetColor {rgba:[0.15,0.7,0.25,1.]});
        tool(&d,LayerCanvasTool::Region {fill:true,source:layer_ui::RegionSource::Editing});
        pen_points(&mut d,&[[650.,625.],[650.,625.]]);
        let filled=until_paths(&d,"a vector face between separate open strokes",|paths|paths.len()==5);
        assert!(active_vector(&d));
        assert_eq!(ui_session(&d.w).engine().document().working.occurrence,Some(target));
        assert_eq!(ui_session(&d.w).engine().document().scene().order(),layers,"vector lines do not create a paint fallback layer");
        let face=filled.iter().find(|path|path.style.strokes().next().is_none()).unwrap();
        assert!(face.encloses([650.,625.],0.05));
        assert_eq!(filled.iter().position(|path|path.id==face.id),Some(original.len()),"the fill sits below its four bounding strokes");
        for path in &original {assert_eq!(filled.iter().find(|candidate|candidate.id==path.id),Some(path));}
        wait_for("the green vector face on screen",||greenish(presented(&d,[650.,625.])));
        assert_eq!(boundary.map(|point|presented(&d,point)),boundary_pixels,"opaque line centres stay above the new face");
        capture(&d,&format!("vector-face-{theme:?}.png"));
        d.w.dispatch(UiAction::SetColor {rgba:[0.1,0.2,0.9,1.]});
        pen_points(&mut d,&[[650.,625.],[650.,625.]]);
        let recoloured=until_paths(&d,"recolouring the same vector face",|paths|paths.len()==5&&paths.iter().find(|path|path.id==face.id).is_some_and(|path|path.style!=face.style));
        for path in &original {assert_eq!(recoloured.iter().find(|candidate|candidate.id==path.id),Some(path));}
        wait_for("the blue vector face on screen",|| {let p=presented(&d,[650.,625.]);i16::from(p[2])-i16::from(p[0]).max(i16::from(p[1]))>40});
        assert_eq!(boundary.map(|point|presented(&d,point)),boundary_pixels,"recolour preserves the opaque line centres");
        capture(&d,&format!("vector-face-recoloured-{theme:?}.png"));
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});
        until_paths(&d,"one Undo restores the original face colour",|paths|paths==filled.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});
        until_paths(&d,"one Undo removes only the face",|paths|paths==original.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});
        until_paths(&d,"Redo restores the vector face",|paths|paths==filled.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});
        until_paths(&d,"the bounding strokes remain exact",|paths|paths==original.as_slice());
        eprintln!("PASS {theme:?}: separate open vector strokes, face Fill, recolour and exact Undo/Redo");
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_gap_fill_and_everywhere --tablet"]
fn native_vector_gap_fill_and_everywhere() {
    let mut d=Driver::new("art.capycanvas.VectorSeals");
    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        add_vector_layer(&mut d);
        d.w.dispatch(UiAction::SelectBrush {id:layer_core::DefaultBrushPreset::GPen as u32});
        d.w.dispatch(UiAction::SetBrushSize {value:6.});d.w.dispatch(UiAction::SetColor {rgba:[0.,0.,0.,1.]});
        tool(&d,LayerCanvasTool::Paint);
        pen_points(&mut d,&[[670.,400.],[900.,400.],[900.,800.],[400.,800.],[400.,400.],[630.,400.]]);
        let original=until_paths(&d,"one open vector outline with a 40 px gap",|paths|paths.len()==1);
        assert!(!original[0].encloses([650.,600.],0.05));
        let target=ui_session(&d.w).engine().document().working.occurrence.unwrap();
        let order=ui_session(&d.w).engine().document().scene().order().to_vec();
        d.w.dispatch(UiAction::SetColor {rgba:[0.15,0.7,0.25,1.]});
        tool(&d,LayerCanvasTool::Region {fill:true,source:layer_ui::RegionSource::Editing});
        d.w.dispatch(UiAction::Invoke {command:CommandId::FillConnected});
        d.w.dispatch(UiAction::Invoke {command:CommandId::CloseGapsAuto});
        assert!(ui_session(&d.w).command(CommandId::CloseGapsAuto).selected,"vector Fill starts with Auto closing");
        let checkpoint=ui_session(&d.w).engine().checkpoint();let at=canvas_point(&d,[650.,600.]);
        d.input.perform(serde_json::json!([{"pen":"move","point":at},{"pen":"down","point":at}]));
        until_paths(&d,"press previews the gappy vector face",|paths|paths.len()==2);
        assert_eq!(ui_session(&d.w).engine().checkpoint(),checkpoint,"press preview creates no history");
        wait_for("vector hold publishes Close gaps",||state(&d.w).fill_gesture.is_some_and(|view|view.pill.is_some()));
        let held=state(&d.w).fill_gesture.unwrap();let pill=held.pill.as_ref().unwrap();
        assert_eq!(pill.label.as_ref(),"Close gaps");assert_eq!(pill.value.as_ref(),"48 px");
        capture(&d,&format!("vector-gap-held-48-{theme:?}.png"));
        let off=gesture_point(&d,[pill.track[0],held.tip[1]]);
        d.input.perform(serde_json::json!([{"pen":"move","point":off}]));
        until_paths(&d,"Off previews no face through the open gap",|paths|paths==original.as_slice());
        assert_eq!(state(&d.w).fill_gesture.unwrap().pill.unwrap().value.as_ref(),"Off");
        assert_eq!(ui_session(&d.w).engine().checkpoint(),checkpoint);
        capture(&d,&format!("vector-gap-held-off-{theme:?}.png"));
        let forty_eight=gesture_point(&d,held.tip);
        d.input.perform(serde_json::json!([{"pen":"move","point":forty_eight}]));
        until_paths(&d,"48 previews the inner face again",|paths|paths.len()==2);
        assert_eq!(state(&d.w).fill_gesture.unwrap().pill.unwrap().value.as_ref(),"48 px");
        d.input.perform(serde_json::json!([{"pen":"up"},{"pen":"leave"}]));
        wait_for("lift commits one vector fill edit",||ui_session(&d.w).engine().checkpoint()!=checkpoint);
        let filled=until_paths(&d,"lift commits the gappy vector face",|paths|paths.len()==2);
        assert!(state(&d.w).tool_extra.iter().any(|option|matches!(option,layer_ui::ToolOption::Choice {id:"close-gaps",items,..} if items.iter().any(|item|item.selected && item.label.as_ref()=="48 px"))),"lift settles the48pxslot");
        let seals=state(&d.w).fill_gesture.unwrap().seals;assert!(!seals.is_empty());
        let [a,b]=seals[0];let seal=gesture_point(&d,[(a[0]+b[0])*0.5,(a[1]+b[1])*0.5]);
        let sealed_checkpoint=ui_session(&d.w).engine().checkpoint();
        let before_seal=paths(&d);
        d.input.perform(serde_json::json!([{"pen":"move","point":seal},{"pen":"down","point":seal}]));
        assert_eq!(paths(&d),before_seal,"a shown-seal press does not edit the artwork");
        assert_eq!(ui_session(&d.w).engine().checkpoint(),sealed_checkpoint);
        d.input.perform(serde_json::json!([{"pen":"up"},{"pen":"leave"}]));
        pump(200);assert_eq!(paths(&d),filled,"removing the only seal without a fallback face changes nothing");
        assert_eq!(ui_session(&d.w).engine().checkpoint(),sealed_checkpoint);
        assert_eq!(filled.iter().find(|path|path.id==original[0].id),Some(&original[0]));
        assert!(filled.iter().any(|path|path.style.strokes().next().is_none()&&path.encloses([650.,600.],0.05)));
        assert_eq!(ui_session(&d.w).engine().document().working.occurrence,Some(target));
        assert_eq!(ui_session(&d.w).engine().document().scene().order(),order);
        wait_for("the Auto-closed face is green",||greenish(presented(&d,[650.,600.])));
        capture(&d,&format!("vector-gap-auto-{theme:?}.png"));
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"Undo removes only the sealed face",|paths|paths==original.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});until_paths(&d,"Redo restores the sealed vector face",|paths|paths==filled.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"the gappy stroke remains exact",|paths|paths==original.as_slice());
        d.w.dispatch(UiAction::SetColor {rgba:[0.,0.,0.,1.]});
        tool(&d,LayerCanvasTool::Figure {shape:layer_core::FigureShape::Rectangle,paint:layer_core::FigurePaint::Outline});
        pen_points(&mut d,&[[340.,340.],[960.,860.]]);
        let framed=until_paths(&d,"the gappy shape has a closed outer frame",|paths|paths.len()==2);
        d.w.dispatch(UiAction::SetColor {rgba:[0.15,0.7,0.25,1.]});
        tool(&d,LayerCanvasTool::Region {fill:true,source:layer_ui::RegionSource::Editing});
        let before_inner=ui_session(&d.w).engine().checkpoint();
        pen_points(&mut d,&[[650.,600.],[650.,600.]]);
        wait_for("inner face lift commits",||ui_session(&d.w).engine().checkpoint()!=before_inner);
        let inner=until_paths(&d,"the inner face fills inside its frame",|paths|paths.len()==3);
        let fill_id=inner.iter().find(|path|path.style.strokes().next().is_none()).unwrap().id;
        let seals=state(&d.w).fill_gesture.unwrap().seals;assert!(!seals.is_empty());
        let [a,b]=seals[0];let seal=gesture_point(&d,[(a[0]+b[0])*0.5,(a[1]+b[1])*0.5]);
        let before_seal=paths(&d);let seal_checkpoint=ui_session(&d.w).engine().checkpoint();
        d.input.perform(serde_json::json!([{"pen":"move","point":seal},{"pen":"down","point":seal}]));
        assert_eq!(paths(&d),before_seal,"a shown-seal press does not edit the artwork");
        assert_eq!(ui_session(&d.w).engine().checkpoint(),seal_checkpoint);
        d.input.perform(serde_json::json!([{"pen":"up"},{"pen":"leave"}]));
        let amended=until_paths(&d,"seal removal refills the outer face",|paths|paths.len()==3&&paths.iter().any(|path|path.id==fill_id&&path.encloses([360.,600.],0.05)));
        capture(&d,&format!("vector-gap-seal-removed-{theme:?}.png"));
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"one Undo removes the amended fill",|paths|paths==framed.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});until_paths(&d,"Redo restores the amended outer face",|paths|paths==amended.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"both outlines remain unchanged",|paths|paths==framed.as_slice());
        add_vector_layer(&mut d);
        d.w.dispatch(UiAction::SetColor {rgba:[0.,0.,0.,1.]});
        tool(&d,LayerCanvasTool::Figure {shape:layer_core::FigureShape::Rectangle,paint:layer_core::FigurePaint::Outline});
        pen_points(&mut d,&[[1100.,400.],[1300.,650.]]);
        let outline=until_paths(&d,"one plain closed outline",|paths|paths.len()==1);
        d.w.dispatch(UiAction::SetColor {rgba:[0.15,0.7,0.25,1.]});
        tool(&d,LayerCanvasTool::Region {fill:true,source:layer_ui::RegionSource::Editing});
        let outline_checkpoint=ui_session(&d.w).engine().checkpoint();let at=canvas_point(&d,[1200.,525.]);
        d.input.perform(serde_json::json!([{"pen":"move","point":at},{"pen":"down","point":at}]));
        assert_eq!(paths(&d),outline,"a closed-outline fill waits for lift");
        assert_eq!(ui_session(&d.w).engine().checkpoint(),outline_checkpoint);
        d.input.perform(serde_json::json!([{"pen":"up"},{"pen":"leave"}]));
        let own_fill=until_paths(&d,"the outline gains its own fill",|paths|paths.len()==1&&paths[0].style.entries.iter().any(|entry|matches!(entry,layer_core::authored::vector::PaintEntry::Fill(_))));
        assert_eq!(own_fill[0].id,outline[0].id);assert_eq!(own_fill[0].geometry,outline[0].geometry);
        assert!(matches!(own_fill[0].style.entries[..],[layer_core::authored::vector::PaintEntry::Fill(_),layer_core::authored::vector::PaintEntry::Stroke(_)]));
        capture(&d,&format!("vector-closed-own-fill-{theme:?}.png"));
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"one Undo restores the plain outline",|paths|paths==outline.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});until_paths(&d,"Redo restores the outline's own fill",|paths|paths==own_fill.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"the plain outline remains exact",|paths|paths==outline.as_slice());
        add_vector_layer(&mut d);
        for index in 0..4 {
            d.w.dispatch(UiAction::SetColor {rgba:if index==3 {[0.1,0.2,0.9,1.]}else{[0.15,0.7,0.25,1.]}});
            tool(&d,LayerCanvasTool::Figure {shape:layer_core::FigureShape::Rectangle,paint:layer_core::FigurePaint::Fill});
            let x=350.+index as f32*200.;pen_points(&mut d,&[[x,400.],[x+150.,550.]]);
            until_paths(&d,"four filled vector regions",|paths|paths.len()==index+1);
        }
        let original=paths(&d);assert!(original.iter().all(|path|path.style.strokes().next().is_none()));
        let different=original.iter().find(|path|path.encloses([1025.,475.],0.05)).unwrap();
        let target=ui_session(&d.w).engine().document().working.occurrence.unwrap();
        let order=ui_session(&d.w).engine().document().scene().order().to_vec();
        wait_for("the different region is blue",|| {let p=presented(&d,[1025.,475.]);i16::from(p[2])-i16::from(p[0]).max(i16::from(p[1]))>40});
        let blue=presented(&d,[1025.,475.]);
        tool(&d,LayerCanvasTool::Region {fill:true,source:layer_ui::RegionSource::Editing});
        d.w.dispatch(UiAction::Invoke {command:CommandId::FillEverywhere});assert!(ui_session(&d.w).command(CommandId::FillEverywhere).selected);
        d.w.dispatch(UiAction::SetColor {rgba:[0.85,0.1,0.05,1.]});
        let recolour_checkpoint=ui_session(&d.w).engine().checkpoint();let at=canvas_point(&d,[425.,475.]);
        d.input.perform(serde_json::json!([{"pen":"move","point":at},{"pen":"down","point":at}]));
        assert_eq!(paths(&d),original,"Everywhere recolouring waits for lift");
        assert_eq!(ui_session(&d.w).engine().checkpoint(),recolour_checkpoint);
        d.input.perform(serde_json::json!([{"pen":"up"},{"pen":"leave"}]));
        let recoloured=until_paths(&d,"Everywhere recolours the three matching fills",|paths|paths.len()==4&&original.iter().filter(|old|paths.iter().find(|path|path.id==old.id).is_some_and(|path|path.style!=old.style)).count()==3);
        assert_eq!(recoloured.iter().find(|path|path.id==different.id),Some(different));
        for old in &original {assert_eq!(recoloured.iter().find(|path|path.id==old.id).unwrap().geometry,old.geometry);}
        assert_eq!(ui_session(&d.w).engine().document().working.occurrence,Some(target));assert_eq!(ui_session(&d.w).engine().document().scene().order(),order);
        wait_for("all three matching regions are red",||[425.,625.,825.].into_iter().all(|x| {let p=presented(&d,[x,475.]);i16::from(p[0])-i16::from(p[1]).max(i16::from(p[2]))>40}));
        assert_eq!(presented(&d,[1025.,475.]),blue,"Everywhere preserves the different-colour region");
        capture(&d,&format!("vector-everywhere-{theme:?}.png"));
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"one Undo restores all three original fills",|paths|paths==original.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});until_paths(&d,"one Redo restores the Everywhere edit",|paths|paths==recoloured.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"all original vector regions remain exact",|paths|paths==original.as_slice());
        eprintln!("PASS {theme:?}: 40 px vector gap press/hold Off/48, seal exclusion with and without fallback, Everywhere recolours three matching fills; exact one-edit Undo/Redo");
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_trace_raster_and_enclose --tablet"]
fn native_vector_trace_raster_and_enclose() {
    let mut d=Driver::new("art.capycanvas.VectorTrace");
    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        let size=ui_session(&d.w).engine().document().composition().size;
        let source=layer_core::color::source::rgba8_source(size,|x,y| {
            if (400..900).contains(&x)&&(400..850).contains(&y)&&(x<406||x>=894||y<406||y>=844) {[0,0,0,255]}else{[0;4]}
        });
        ui_session_mut(&d.w).import_layer_source("Raster face lines",std::sync::Arc::unwrap_or_clone(source)).unwrap();d.w.wake();
        new_photo::ready(&d.w);
        let reference=ui_session(&d.w).engine().document().working.occurrence.unwrap();
        let original_reference=ui_session(&d.w).engine().document().scene().paint_source(reference).unwrap().clone();
        d.w.dispatch(UiAction::Layer {action:LayerAction::ReferenceSelection});
        add_vector_layer(&mut d);
        let target=ui_session(&d.w).engine().document().working.occurrence.unwrap();
        let layers=ui_session(&d.w).engine().document().scene().order().to_vec();
        let empty=paths(&d);assert!(empty.is_empty());
        d.w.dispatch(UiAction::SetColor {rgba:[0.15,0.7,0.25,1.]});
        tool(&d,LayerCanvasTool::Region {fill:true,source:layer_ui::RegionSource::Reference});
        d.w.dispatch(UiAction::Invoke {command:CommandId::CloseGapsOff});
        d.w.dispatch(UiAction::Invoke {command:CommandId::EdgesExact});
        pen_points(&mut d,&[[650.,625.],[650.,625.]]);
        let traced=until_paths(&d,"one traced vector fill from raster lines",|paths|paths.len()==1);
        assert!(traced[0].style.strokes().next().is_none()&&traced[0].encloses([650.,625.],0.05));
        assert!(!traced[0].encloses([950.,625.],0.05));
        assert_eq!(ui_session(&d.w).engine().document().working.occurrence,Some(target));
        assert_eq!(ui_session(&d.w).engine().document().scene().order(),layers);
        assert_eq!(ui_session(&d.w).engine().document().scene().paint_source(reference),Some(&original_reference));
        wait_for("the traced fill is green",||greenish(presented(&d,[650.,625.])));
        capture(&d,&format!("vector-raster-trace-{theme:?}.png"));
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});
        until_paths(&d,"one Undo removes the traced vector fill",|paths|paths==empty.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});
        until_paths(&d,"Redo restores the traced vector fill",|paths|paths==traced.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});
        until_paths(&d,"the vector colour layer is empty again",|paths|paths.is_empty());
        add_vector_layer(&mut d);
        let target=ui_session(&d.w).engine().document().working.occurrence.unwrap();
        d.w.dispatch(UiAction::SelectBrush {id:layer_core::DefaultBrushPreset::GPen as u32});
        d.w.dispatch(UiAction::SetBrushSize {value:6.});d.w.dispatch(UiAction::SetColor {rgba:[0.,0.,0.,1.]});
        tool(&d,LayerCanvasTool::Paint);
        for (index,(from,to)) in [([350.,400.],[1050.,400.]),([350.,700.],[1050.,700.]),([400.,350.],[400.,750.]),([600.,350.],[600.,750.]),([800.,350.],[800.,750.]),([1000.,350.],[1000.,750.])].into_iter().enumerate() {
            pen_points(&mut d,&stroke(from,to));until_paths(&d,"three vector faces",|paths|paths.len()==index+1);
        }
        let original=paths(&d);let layers=ui_session(&d.w).engine().document().scene().order().to_vec();
        d.w.dispatch(UiAction::SetColor {rgba:[0.15,0.7,0.25,1.]});
        tool(&d,LayerCanvasTool::EncloseFill {source:layer_ui::RegionSource::Editing});
        pen_points(&mut d,&[[380.,380.],[850.,380.],[850.,720.],[380.,720.],[380.,380.]]);
        let enclosed=until_paths(&d,"two wholly enclosed vector faces",|paths|paths.len()==8);
        let fills=enclosed.iter().filter(|path|path.style.strokes().next().is_none()).collect::<Vec<_>>();assert_eq!(fills.len(),2);
        assert!(fills.iter().any(|path|path.encloses([500.,550.],0.05))&&fills.iter().any(|path|path.encloses([700.,550.],0.05)));
        assert!(fills.iter().all(|path|!path.encloses([900.,550.],0.05)),"the grazed third face stays empty");
        for path in &original {assert_eq!(enclosed.iter().find(|candidate|candidate.id==path.id),Some(path));}
        assert_eq!(ui_session(&d.w).engine().document().working.occurrence,Some(target));
        assert_eq!(ui_session(&d.w).engine().document().scene().order(),layers);
        wait_for("both enclosed vector faces are visible",||greenish(presented(&d,[500.,550.]))&&greenish(presented(&d,[700.,550.])));
        assert!(!greenish(presented(&d,[900.,550.])));
        capture(&d,&format!("vector-enclose-{theme:?}.png"));
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"one Undo removes both faces",|paths|paths==original.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});until_paths(&d,"Redo restores both vector faces",|paths|paths==enclosed.as_slice());
        d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});until_paths(&d,"the vector outlines remain exact",|paths|paths==original.as_slice());
        eprintln!("PASS {theme:?}: raster lines trace into the same vector layer; Enclose fills only contained vector faces; exact Undo/Redo");
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_color_and_canvas_changes --tablet"]
fn native_vector_color_and_canvas_changes() {
    use layer_core::{FigurePaint, FigureShape};
    let mut d = Driver::new("art.capycanvas.VectorColor");
    add_vector_layer(&mut d);
    tool(&d, LayerCanvasTool::Figure { shape: FigureShape::Ellipse, paint: FigurePaint::Outline });
    pen_points(&mut d, &[[400., 300.], [600., 420.], [800., 540.]]);
    let ellipse = until_paths(&d, "an ellipse outline", |paths| paths.len() == 1)[0].clone();
    d.w.dispatch(UiAction::SetColor { rgba: [0.15, 0.7, 0.25, 1.] });
    tool(&d, LayerCanvasTool::Region { fill: true, source: layer_ui::RegionSource::Visible });
    pen_points(&mut d, &[[600., 420.], [600., 420.]]);
    until_paths(&d, "filling inside the outline", |paths| paths.iter().any(|p| p.id == ellipse.id && p.style.entries.len() == 2));
    wait_for("the fill on screen", || greenish(presented(&d, [600., 460.])));
    d.capture_canvas("color-inside.png");
    let order=ui_session(&d.w).engine().document().scene().order().to_vec();
    pen_points(&mut d, &[[1100., 700.], [1100., 700.]]);
    until_paths(&d,"a traced outside fill on the vector layer",|paths|paths.len()==2);
    assert!(active_vector(&d));assert_eq!(ui_session(&d.w).engine().document().scene().order(),order);
    wait_for("the traced outside fill is visible",||greenish(presented(&d,[1100.,760.])));
    assert!(greenish(presented(&d, [600., 460.])), "the path keeps its fill");
    capture(&d, "color-outside.png");

    let extent = ui_session(&d.w).engine().document().composition().size;
    d.w.dispatch(UiAction::Invoke { command: CommandId::RotateImageRight });
    wait_for("the canvas turns", || ui_session(&d.w).engine().document().composition().size == [extent[1], extent[0]]);
    let turned = [extent[1] as f32 - 420., 600.];
    wait_for("the filled ellipse turns with the canvas", || greenish(presented(&d, turned)));
    capture(&d, "canvas-rotated.png");
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_vector_select_erase_mask_and_merge --tablet"]
fn native_vector_select_erase_mask_and_merge() {
    let mut d = Driver::new("art.capycanvas.VectorEdit");
    d.w.dispatch(UiAction::SetColor { rgba: [0.1, 0.1, 0.12, 1.] });
    add_vector_layer(&mut d);
    d.w.dispatch(UiAction::SetBrushSize { value: 14. });
    pump(100);
    for (from, to) in [([300., 400.], [1100., 400.]), ([500., 200.], [500., 600.]), ([900., 200.], [900., 600.])] { pen_points(&mut d, &stroke(from, to)); pump(150); }
    until_paths(&d, "three crossing lines", |paths| paths.len() == 3);
    d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x61,"down":true},{"key":0x61,"down":false},{"key":0xffe3,"down":false}]));
    wait_for("Select All selects every path", || ui_session(&d.w).engine().document().working.vector_items.as_ref().is_some_and(|s| s.items.len() == 3));
    d.capture_canvas("select-all.png");
    d.w.dispatch(UiAction::Invoke { command: CommandId::TransformFlipVertical });
    pump(200);
    d.input.key(0xff1b);
    wait_for("Escape clears the paths", || ui_session(&d.w).engine().document().working.vector_items.is_none());

    d.w.dispatch(UiAction::Invoke { command: CommandId::Eraser });
    pump(200);
    d.w.dispatch(UiAction::SetBrushSize { value: 12. });
    let label = CommandId::EraseToIntersection.localized_label(ui_session(&d.w).localization()).to_string();
    d.click_label(&label);
    wait_for("Erase up to intersection", || state(&d.w).settings.vector_eraser == layer_ui::VectorEraser::ToIntersection);
    capture(&d, "erase-modes.png");
    pump(150);
    pen_points(&mut d, &stroke([700., 380.], [700., 420.]));
    until_paths(&d, "erasing up to the crossings", |paths| paths.len() == 4);
    wait_for("the span between the crossings is gone", || !inked(presented(&d, [800., 400.])) && inked(presented(&d, [400., 400.])) && inked(presented(&d, [1000., 400.])));
    d.capture_canvas("erase-to-intersection.png");

    let layer = ui_session(&d.w).engine().document().working.occurrence.unwrap();
    let id = occurrence_token(layer);
    let before = paths(&d);
    d.w.dispatch(UiAction::Layer { action: LayerAction::AddMask { id, replace: false } });
    d.w.dispatch(UiAction::Layer { action: LayerAction::Select { id, mask: true } });
    d.w.dispatch(UiAction::Invoke { command: CommandId::Eraser });
    pump(200);
    let revision = ui_session(&d.w).engine().document().revision;
    pen_points(&mut d, &stroke([400., 300.], [400., 500.]));
    wait_for("the mask stroke", || ui_session(&d.w).engine().document().revision != revision);
    assert_eq!(paths(&d), before, "mask strokes leave the paths");
    wait_for("the mask hides the left stub", || !inked(presented(&d, [400., 400.])) && inked(presented(&d, [1000., 400.])));
    d.capture_canvas("mask.png");
    d.w.dispatch(UiAction::Layer { action: LayerAction::Select { id, mask: false } });
    d.w.dispatch(UiAction::Layer { action: LayerAction::NewBelow });
    d.w.dispatch(UiAction::Layer { action: LayerAction::Select { id, mask: false } });
    pump(300);
    d.w.dispatch(UiAction::Invoke { command: CommandId::MergeDown });
    press_notice(&mut d, layer_ui::NoticeActionId::Merge);
    wait_for("the lines become pixels", || !active_vector(&d) && ui_session(&d.w).engine().document().scene().vector_layer(layer).is_none());
    wait_for("the merged pixels keep the mask", || !inked(presented(&d, [400., 400.])) && inked(presented(&d, [1000., 400.])));
    d.capture_canvas("merged.png");
    d.finish();
}
