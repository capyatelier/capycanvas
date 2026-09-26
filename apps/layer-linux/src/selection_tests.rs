//! GTK selection drawer, actual compositor contacts, and committed GPU masks.
use super::*;
use layer_ui::SelectionTool;

fn selection(d: &Driver) -> Option<layer_core::Selection> {
    d.w.gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .selection
        .clone()
}
fn canvas_point(d: &Driver, p: [f32; 2]) -> [f32; 2] {
    let m = state(&d.w).camera.document_to_surface();
    let scale = d.w.area.scale_factor() as f32;
    let p = gtk::graphene::Point::new(
        (m[0] * p[0] + m[2] * p[1] + m[4]) / scale,
        (m[1] * p[0] + m[3] * p[1] + m[5]) / scale,
    );
    let p = d.w.area.compute_point(&d.w.window, &p).unwrap();
    [p.x(), p.y()]
}
fn canvas_click(d: &mut Driver, p: [f32; 2]) {
    let p = canvas_point(d, p);
    d.perform(serde_json::json!([{"point":p,"down":true},{"down":false}]));
}
fn canvas_drag(d: &mut Driver, a: [f32; 2], b: [f32; 2], pen: bool) {
    let a = canvas_point(d, a);
    let b = canvas_point(d, b);
    d.perform(if pen {serde_json::json!([{"pen":"move","point":a},{"pen":"down","point":a},{"pen":"move","point":b},{"pen":"up"},{"pen":"leave"}])}
        else {serde_json::json!([{"point":a,"down":true},{"point":b},{"down":false}])});
}
fn wait_selection(d: &Driver) -> layer_core::Selection {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        pump(20);
        if let Some(selection) = selection(d) {
            return selection;
        }
        assert!(
            Instant::now() < deadline,
            "selection timed out: {}",
            d.w.status.text()
        );
    }
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_quick_mask_input"]
fn native_quick_mask_input() {
    let mut d = Driver::new("art.capycanvas.QuickMask");
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| d.dir.to_string_lossy().into()));
    std::fs::create_dir_all(&output).unwrap();
    d.key(b'q' as u32);
    assert!(state(&d.w).layer_tools.quick_mask);
    assert!(selection(&d).is_none(), "entering is display state only");
    assert_eq!(state(&d.w).layer_properties.controls.len(),3);
    d.w.dispatch(UiAction::SetBrushSize { value: 80. });
    let _=crate::snapshot(&d.w); pump(100);
    crate::snapshot(&d.w).save_to_png(output.join("quick-mask-before.png")).unwrap();
    canvas_drag(&mut d, [600.,600.], [900.,600.], false);
    let first = wait_selection(&d);
    assert!(byte_pixel(&first,750,600) > 200);
    assert_eq!(byte_pixel(&first,100,100),0);
    let layers = d.header_tool(ToolbarControl::Panel { panel:Panel::Layers });
    d.click_name(&layers); pump(150);
    assert!(d.named("art-layer-0").is_visible());
    pump(500);
    let row = d.named("art-layer-0");
    let thumbnail = find_css(&row, "layer-thumbnail").unwrap();
    fn picture(widget: &gtk::Widget) -> Option<gtk::Picture> {
        if let Ok(p) = widget.clone().downcast::<gtk::Picture>() { return Some(p); }
        let mut child = widget.first_child();
        while let Some(w) = child { if let Some(p) = picture(&w) { return Some(p); } child = w.next_sibling(); }
        None
    }
    assert!(picture(&thumbnail).unwrap().paintable().is_some(), "Quick Mask uses the normal GPU thumbnail");

    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme:Some(theme) }); pump(250);
        let _ = crate::snapshot(&d.w); pump(100);
        crate::snapshot(&d.w).save_to_png(output.join(format!("quick-mask-{theme:?}.png"))).unwrap();
        let capture=d.w.gpu.borrow().as_ref().unwrap().session.engine().backend().capture().unwrap();
        let tinted=capture.bytes.chunks_exact(4).filter(|p|p[0]>p[1].saturating_add(30) && p[0]>p[2].saturating_add(30)).count();
        assert!(tinted>100,"{theme:?} mask overlay disappeared: {tinted} pixels");
    }
    d.click_name(&layers); pump(100);
    let properties = d.header_tool(ToolbarControl::Panel { panel:Panel::Adjustments });
    d.click_name(&properties); pump(150);
    d.number(&d.named("property-mask_opacity"), "40");
    assert_eq!(state(&d.w).layer_properties.controls.iter().find(|c| c.key=="mask_opacity").unwrap().value,layer_core::EffectValue::Number(0.4));
    let _ = crate::snapshot(&d.w); pump(100);
    crate::snapshot(&d.w).save_to_png(output.join("quick-mask-properties.png")).unwrap();
    d.click_name(&properties); d.click_name(&layers); pump(100);
    d.click_name("selection-load-0"); pump(100);
    d.click_name(&layers); pump(100);
    assert!(!state(&d.w).layer_tools.quick_mask);
    assert_eq!(selection(&d),Some(first.clone()));
    d.w.dispatch(UiAction::Invoke {command:CommandId::SaveSelectionLayer}); pump(150);
    let id = state(&d.w).layers.iter().find(|l|l.selection_layer).unwrap().id;
    d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::CancelRename});
    pump(100);
    d.click_name(&layers); pump(100);
    let name = find_css(&d.named(&format!("art-layer-{id}")), "layer-name").unwrap();
    let point = d.point(&name);
    d.perform(serde_json::json!([{"point":point},{"down":true},{"down":false},{"down":true},{"down":false}]));
    assert_eq!(state(&d.w).layer_tools.rename_layer,Some(id), "double-clicking the name edits it");
    d.key(0xff1b);
    assert_eq!(state(&d.w).layer_tools.mask_editing.unwrap().layer,Some(id));
    d.click_name(&layers); pump(100);
    let saved = || d.w.gpu.borrow().as_ref().unwrap().session.engine().document().saved_selection(layer_core::LayerId(id)).unwrap();
    let before = saved();
    d.w.dispatch(UiAction::Selection {action:layer_ui::SelectionAction::BeginResize {grow:true,layer:Some(id)}}); pump(100);
    d.number(&d.named("selection-resize-distance"), "8");
    d.click_label("Apply");
    let deadline = Instant::now()+Duration::from_secs(30);
    while d.w.gpu.borrow().as_ref().unwrap().session.engine().document().saved_selection(layer_core::LayerId(id)).unwrap()==before {
        assert!(Instant::now()<deadline,"Grow completed");pump(20);
    }
    assert!(state(&d.w).layer_tools.selection_resize.is_none());
    d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});pump(100);
    assert_eq!(d.w.gpu.borrow().as_ref().unwrap().session.engine().document().saved_selection(layer_core::LayerId(id)).unwrap(),before);

    d.w.dispatch(UiAction::Invoke {command:CommandId::ClearSelectionMask}); pump(100);
    assert_eq!(selection(&d),Some(first));
    d.w.dispatch(UiAction::Selection {action:layer_ui::SelectionAction::LoadLayer {id,mode:layer_ui::SelectionMode::New,inverted:false}}); pump(100);
    assert_eq!(selection(&d),Some(layer_core::Selection::empty()));
    assert!(state(&d.w).layer_tools.mask_editing.is_none());
    assert!(state(&d.w).host_error.is_none(),"{:?}",state(&d.w).host_error);
    d.click_name(&layers); pump(150);
    let _ = crate::snapshot(&d.w); pump(100);
    crate::snapshot(&d.w).save_to_png(output.join("selection-layer.png")).unwrap();
    d.w.window.destroy(); pump(80);
}
fn pixel(s: &layer_core::Selection, x: u32, y: u32) -> u32 {
    let layer_core::SelectionShape::Pixels(p) = &s.shape else {
        panic!("pixel selection")
    };
    (p.words()[(y * p.extent()[0].div_ceil(8) + x / 8) as usize] >> ((x % 8) * 4)) & 15
}

#[test]
#[ignore = "isolated native-input.js --tablet --native-test=native_selection_brush_input"]
fn native_selection_brush_input() {
    let mut d=Driver::new("art.capycanvas.SelectionBrush");
    let output=std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_|d.dir.to_string_lossy().into()));
    std::fs::create_dir_all(&output).unwrap();
    let opener=d.header_tool(ToolbarControl::Command {command:CommandId::Select});
    d.click_name(&opener);
    if state(&d.w).customization.drawer.is_none() { d.click_name(&opener); }
    d.click_name("tool-choice-SelectionBrush");
    let panel=d.named("drawer-panel-ToolSettings");
    for id in ["selection_brush_size","selection_brush_hardness","selection_brush_opacity"] {
        let control=d.named(&format!("tool-setting-{id}"));
        let bounds=control.compute_bounds(&panel).unwrap();
        assert!(bounds.width()>60. && bounds.height()>=24.,"{id}: {bounds:?}");
    }
    assert_eq!(state(&d.w).tool_actions.len(),3);
    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)}); pump(250);
        let _=crate::snapshot(&d.w); pump(100);
        crate::snapshot(&d.w).save_to_png(output.join(format!("selection-brush-{theme:?}.png"))).unwrap();
    }
    let tablet=std::env::var("WAYLAND_DISPLAY").is_ok_and(|s|s=="layer-bench-tablet");
    if tablet {
        // Text-field keymap delivery is covered by the plain Wayland journey.
        d.w.dispatch(UiAction::SetToolSetting {id:"selection_brush_opacity".into(),value:0.5});
    } else { d.number(&d.named("tool-setting-selection_brush_opacity"),"50"); }
    d.click_name(&opener);
    let wait_value=|d:&Driver,x:u32,y:u32,expected:u32| {
        let deadline=Instant::now()+Duration::from_secs(20);
        loop {
            pump(20);
            if let Some(s)=selection(d) && let layer_core::SelectionShape::Pixels(p)=&s.shape {
                let value=(p.words()[(y*p.extent()[0].div_ceil(4)+x/4) as usize]>>((x%4)*8))&255;
                if value==expected { return s; }
            }
            assert!(Instant::now()<deadline,"selection value timed out: {}",d.w.status.text());
        }
    };
    canvas_drag(&mut d,[600.,600.],[850.,600.],false);
    let first=wait_value(&d,700,600,128);
    canvas_drag(&mut d,[600.,600.],[850.,600.],tablet);
    wait_value(&d,700,600,192);
    d.w.dispatch(UiAction::Invoke {command:CommandId::Undo}); pump(200);
    assert_eq!(selection(&d),Some(first));
    d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});
    wait_value(&d,700,600,192);
    d.w.dispatch(UiAction::Invoke {command:CommandId::Deselect}); pump(150);
    let points=[[600.,600.],[900.,600.],[900.,900.],[600.,900.],[601.,601.]];
    let mut actions=Vec::new();
    for (i,p) in points.into_iter().enumerate() {
        let p=canvas_point(&d,p);
        actions.push(if i==0 {serde_json::json!({"point":p,"down":true})} else {serde_json::json!({"point":p})});
    }
    actions.push(serde_json::json!({"down":false}));
    d.perform(serde_json::Value::Array(actions));
    wait_value(&d,750,750,128);
    let _=crate::snapshot(&d.w); pump(100);
    crate::snapshot(&d.w).save_to_png(output.join("selection-brush-canvas.png")).unwrap();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_selection_tools_input"]
fn native_selection_tools_input() {
    let mut d = Driver::new("art.capycanvas.SelectionTools");
    let output = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| d.dir.to_string_lossy().into()),
    );
    std::fs::create_dir_all(&output).unwrap();
    let opener = d.header_tool(ToolbarControl::Command {
        command: CommandId::Select,
    });
    d.click_name(&opener);
    if state(&d.w).customization.drawer.is_none() {
        d.click_name(&opener);
    }
    let tools = d.named("drawer-panel-Tools");
    let settings = d.named("drawer-panel-ToolSettings");
    let select_icon = d.header_icon(CommandId::Select, "lasso");
    assert!(find_named(&d.named("tool-drawer"), "drawer-panel-Brushes").is_none());
    for (i, tool) in SelectionTool::ALL.into_iter().enumerate() {
        let button = d.named(&format!("tool-choice-{:?}", tool.command()));
        assert!(
            button.compute_bounds(&tools).unwrap().height() >= 32.,
            "compact selection tool row"
        );
        let p = d.point(&button);
        match i % 3 {
            0 => d.click(&button),
            1 => d.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}])),
            _ => d.click(&button),
        }
        assert_eq!(state(&d.w).layer_tools.tool.selection_tool(), Some(tool));
        assert_eq!(d.header_icon(CommandId::Select, tool.command().icon().unwrap()), select_icon);
        assert!(
            state(&d.w).customization.drawer.is_some(),
            "choosing a selection tool keeps the drawer open"
        );
        assert_eq!(d.named("drawer-panel-Tools"), tools);
        assert_eq!(d.named("drawer-panel-ToolSettings"), settings);
        assert_eq!(state(&d.w).tool_panels.tools.subtools.len(), 8);
        assert_shared_icons(&d.named("tool-drawer"));
    }
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let _ = crate::snapshot(&d.w);
        pump(100);
        crate::snapshot(&d.w)
            .save_to_png(output.join(format!("sketch-select-{theme:?}.png")))
            .unwrap();
    }
    d.click_name("tool-choice-RectangleSelect");
    d.click_name("tool-action-SelectionFixedSize");
    d.number(&d.named("tool-setting-selection_width"), "240");
    d.number(&d.named("tool-setting-selection_height"), "120");
    d.click_name("tool-action-SelectionFixedSize"); // free geometry for the canvas journey
    d.click_name(&opener);
    assert!(state(&d.w).customization.drawer.is_none());
    canvas_drag(&mut d, [600., 600.], [900., 800.], false);
    let rectangle = wait_selection(&d);
    assert_eq!(rectangle.contours()[0].len(), 4);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(100);
    assert!(selection(&d).is_none());
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    pump(100);
    assert_eq!(selection(&d), Some(rectangle.clone()));
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::EllipseSelect,
    });
    canvas_drag(&mut d, [650., 600.], [950., 850.], false);
    let ellipse = wait_selection(&d);
    assert!(ellipse.contours()[0].len() > 16);
    assert_ne!(ellipse, rectangle);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::PolygonSelect,
    });
    for point in [[600., 600.], [900., 600.], [900., 900.]] {
        canvas_click(&mut d, point);
    }
    assert_eq!(
        selection(&d),
        Some(ellipse.clone()),
        "polygon is not committed until closed"
    );
    d.key(0xff08); // Backspace
    canvas_click(&mut d, [800., 850.]);
    d.key(0xff0d); // Enter
    let polygon = wait_selection(&d);
    assert_eq!(polygon.contours()[0].len(), 3);
    assert_ne!(polygon, ellipse);
    canvas_click(&mut d, [650., 650.]);
    d.key(0xff1b);
    assert_eq!(selection(&d), Some(polygon));

    // Separate patches with identical colors prove global selection differs from the wand.
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Deselect,
    });
    d.w.dispatch(UiAction::SetColor {
        rgba: [0.85, 0.1, 0.1, 1.],
    });
    d.w.dispatch(UiAction::SetBrushOpacity { value: 1. });
    for (a, b) in [([500., 500.], [700., 700.]), ([1100., 500.], [1300., 700.])] {
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::RectangleSelect,
        });
        canvas_drag(&mut d, a, b, false);
        wait_selection(&d);
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::FillSelection,
        });
        pump(500);
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::Deselect,
        });
    }
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::AutoSelect,
    });
    canvas_click(&mut d, [600., 600.]);
    let wand = wait_selection(&d);
    assert_eq!(pixel(&wand, 600, 600), 4);
    assert_eq!(pixel(&wand, 1200, 600), 0);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Deselect,
    });
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::ColorSelect,
    });
    canvas_click(&mut d, [600., 600.]);
    let color = wait_selection(&d);
    assert_eq!(pixel(&color, 600, 600), 4);
    assert_eq!(pixel(&color, 1200, 600), 4);
    assert_eq!(pixel(&color, 900, 600), 0);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(100);
    assert!(selection(&d).is_none());
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    pump(100);
    assert_eq!(selection(&d), Some(color));

    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Photographer.layout(Platform::Gtk),
            ..WorkspaceState::default()
        }),
    });
    pump(500);
    for command in [
        CommandId::RectangleSelect,
        CommandId::EllipseSelect,
        CommandId::PolygonSelect,
        CommandId::ColorSelect,
    ] {
        let tile = state(&d.w)
            .workspace
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()
            .iter()
            .find(|t| t.control == ToolbarControl::Command { command })
            .unwrap()
            .id;
        let button = d.named(&format!("tile-{tile}"));
        d.click(&button);
        assert_eq!(
            state(&d.w)
                .layer_tools
                .tool
                .selection_tool()
                .unwrap()
                .command(),
            command
        );
    }
    // Leave a useful review capture showing the new toolbar and color selection.
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let _ = crate::snapshot(&d.w);
        pump(100);
        crate::snapshot(&d.w)
            .save_to_png(output.join(format!("photo-selection-{theme:?}.png")))
            .unwrap();
    }
    assert!(state(&d.w).host_error.is_none());
    d.w.window.destroy();
    pump(80);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_selection_pen_input --tablet"]
fn native_selection_pen_input() {
    let mut d = Driver::new("art.capycanvas.SelectionPen");
    let opener = d.header_tool(ToolbarControl::Command {
        command: CommandId::Select,
    });
    d.click_name(&opener);
    if state(&d.w).customization.drawer.is_none() {
        d.click_name(&opener);
    }
    for tool in SelectionTool::ALL {
        let button = d.named(&format!("tool-choice-{:?}", tool.command()));
        let p = d.point(&button);
        d.perform(serde_json::json!([{"pen":"move","point":p},{"pen":"down"},{"pen":"up"},{"pen":"leave"}]));
        assert_eq!(state(&d.w).layer_tools.tool.selection_tool(), Some(tool));
        assert!(state(&d.w).customization.drawer.is_some());
        d.header_icon(CommandId::Select, tool.command().icon().unwrap());
    }
    d.click_name("tool-choice-RectangleSelect");
    for command in [CommandId::SelectionAdd, CommandId::SelectionSubtract, CommandId::SelectionIntersect, CommandId::SelectionNew] {
        let button = d.named(&format!("tool-action-{command:?}"));
        let p = d.point(&button);
        d.perform(serde_json::json!([{"pen":"move","point":p},{"pen":"down"},{"pen":"up"},{"pen":"leave"}]));
        assert!(button.downcast_ref::<gtk::ToggleButton>().unwrap().is_active());
        assert!(state(&d.w).commands.iter().any(|c| c.id == command && c.selected));
    }
    let mut select_icon = CommandId::RectangleSelect.icon().unwrap();
    for (command, choice, icon) in [
        (CommandId::DrawingBrush, "brush-set-pencil", "pencil"),
        (CommandId::Sculpt, "sculpt-set-liquify", "liquify"),
        (CommandId::Select, "tool-choice-ColorSelect", "color-select"),
    ] {
        let p = d.point(&d.named(&d.header_tool(ToolbarControl::Command { command })));
        d.perform(serde_json::json!([{"pen":"move","point":p},{"pen":"down"},{"pen":"up"},{"pen":"leave"}]));
        let p = d.point(&d.named(choice));
        d.perform(serde_json::json!([{"pen":"move","point":p},{"pen":"down"},{"pen":"up"},{"pen":"leave"}]));
        d.header_icon(command, icon);
        if command == CommandId::Select {
            select_icon = icon;
        }
        d.header_icon(CommandId::Select, select_icon);
    }
    d.header_icon(CommandId::DrawingBrush, "pencil");
    d.header_icon(CommandId::Sculpt, "liquify");
    d.click_name(&opener);
    for tool in [SelectionTool::Rectangle, SelectionTool::Ellipse] {
        d.w.dispatch(UiAction::Invoke {
            command: tool.command(),
        });
        canvas_drag(&mut d, [600., 600.], [900., 800.], true);
        let shape = wait_selection(&d);
        assert_eq!(
            shape.contours()[0].len() == 4,
            tool == SelectionTool::Rectangle
        );
    }
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::PolygonSelect,
    });
    for point in [[600., 600.], [900., 600.], [850., 850.], [600., 600.]] {
        canvas_drag(&mut d, point, point, true);
    }
    assert_eq!(wait_selection(&d).contours()[0].len(), 3);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Deselect,
    });
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::ColorSelect,
    });
    canvas_drag(&mut d, [700., 700.], [700., 700.], true);
    let color = wait_selection(&d);
    assert!(matches!(color.shape, layer_core::SelectionShape::Pixels(_)));
    assert!(state(&d.w).host_error.is_none());
    d.w.window.destroy();
    pump(80);
}

fn byte_pixel(s: &layer_core::Selection, x: u32, y: u32) -> u32 {
    let layer_core::SelectionShape::Pixels(p) = &s.shape else {
        panic!("pixel selection")
    };
    assert_eq!(p.coverage_format(), 2);
    (p.words()[(y * p.extent()[0].div_ceil(4) + x / 4) as usize] >> ((x % 4) * 8)) & 255
}
fn wait_changed_selection(d: &Driver, old: &layer_core::Selection) -> layer_core::Selection {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        pump(20);
        if let Some(selection) = selection(d)
            && &selection != old
        {
            return selection;
        }
        assert!(
            Instant::now() < deadline,
            "selection options timed out: {}",
            d.w.status.text()
        );
    }
}
#[test]
#[ignore = "isolated native-input.js --native-test=native_selection_options_input"]
fn native_selection_options_input() {
    let mut d = Driver::new("art.capycanvas.SelectionOptions");
    let opener = d.header_tool(ToolbarControl::Command {
        command: CommandId::Select,
    });
    d.click_name(&opener);
    if state(&d.w).customization.drawer.is_none() {
        d.click_name(&opener);
    }
    for tool in SelectionTool::ALL.into_iter().filter(|t| !matches!(t, SelectionTool::Brush | SelectionTool::Tonal)) {
        d.click_name(&format!("tool-choice-{:?}", tool.command()));
        assert!(d.named("tool-setting-selection_feather").is_visible());
        let settings = d.named("drawer-panel-ToolSettings");
        assert!(find_named(&settings, "selection-tool-help").is_none());
        let row = d.named("selection-mode-row");
        assert!(row.measure(gtk::Orientation::Horizontal, -1).0 as f32
            <= layer_ui::TOOL_PANEL_MIN_WIDTH - 2. * layer_ui::PANEL_CONTENT_INSET,
            "mode row fits the narrowest tool panel");
        let buttons = [CommandId::SelectionNew, CommandId::SelectionAdd, CommandId::SelectionSubtract, CommandId::SelectionIntersect]
            .map(|command| d.named(&format!("tool-action-{command:?}")).downcast::<gtk::ToggleButton>().unwrap());
        for (i, button) in buttons.iter().enumerate() {
            assert!(button.child().is_some_and(|child| child.is::<gtk::Image>()));
            assert!(button.tooltip_text().is_some());
            let bounds = button.compute_bounds(&row).unwrap();
            assert_eq!(bounds.y(), 0., "modes occupy one row");
            if i > 0 { assert!(bounds.x() > buttons[i - 1].compute_bounds(&row).unwrap().x()); }
        }
        assert_shared_icons(&row);
        for (i, command) in [
            CommandId::SelectionAdd,
            CommandId::SelectionSubtract,
            CommandId::SelectionIntersect,
            CommandId::SelectionNew,
        ].into_iter().enumerate() {
            let button = d.named(&format!("tool-action-{command:?}"));
            if i % 2 == 0 {
                let p = d.point(&button);
                d.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
            } else { d.click(&button); }
            assert!(
                state(&d.w)
                    .commands
                    .iter()
                    .find(|c| c.id == command)
                    .unwrap()
                    .selected
            );
            assert!(button.downcast_ref::<gtk::ToggleButton>().unwrap().is_active());
            assert_eq!(buttons.iter().filter(|b| b.is_active()).count(), 1);
            assert_eq!(d.named("selection-mode-row"), row, "changing modes retains the row");
            assert!(state(&d.w).customization.drawer.is_some());
        }
        d.click_name("tool-action-SelectionNew");
        assert!(buttons[0].is_active(), "clicking the active mode keeps it selected");
    }
    d.click_name("tool-choice-RectangleSelect");
    d.number(&d.named("tool-setting-selection_feather"), "8");
    d.click_name(&opener);
    canvas_drag(&mut d, [600., 600.], [900., 800.], false);
    let softened = wait_selection(&d);
    assert_eq!(byte_pixel(&softened, 750, 700), 255);
    assert!((1..255).contains(&byte_pixel(&softened, 599, 700)));
    d.click_name(&opener);
    d.number(&d.named("tool-setting-selection_feather"), "0");
    d.click_name("tool-action-SelectionAntialias");
    d.click_name("tool-action-SelectionAdd");
    d.click_name(&opener);
    canvas_drag(&mut d, [800., 700.], [1100., 900.], false);
    let added = wait_changed_selection(&d, &softened);
    assert_eq!(byte_pixel(&added, 750, 700), 255);
    assert_eq!(byte_pixel(&added, 1000, 800), 255);
    d.click_name(&opener);
    d.click_name("tool-action-SelectionSubtract");
    d.click_name(&opener);
    canvas_drag(&mut d, [800., 650.], [1000., 900.], false);
    let subtracted = wait_changed_selection(&d, &added);
    assert_eq!(byte_pixel(&subtracted, 850, 750), 0);
    assert_eq!(byte_pixel(&subtracted, 750, 700), 255);
    d.click_name(&opener);
    d.click_name("tool-action-SelectionIntersect");
    d.click_name(&opener);
    canvas_drag(&mut d, [600., 650.], [850., 750.], false);
    let intersection = wait_changed_selection(&d, &subtracted);
    assert_eq!(byte_pixel(&intersection, 750, 700), 255);
    assert_eq!(byte_pixel(&intersection, 1050, 800), 0);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    pump(100);
    assert_eq!(selection(&d), Some(subtracted));
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    pump(100);
    assert_eq!(selection(&d), Some(intersection));
    d.click_name(&opener);
    d.click_name("tool-choice-ColorSelect");
    d.click_name("tool-action-SelectionAntialias");
    let output = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| d.dir.to_string_lossy().into()),
    );
    std::fs::create_dir_all(&output).unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let _ = crate::snapshot(&d.w);
        pump(100);
        crate::snapshot(&d.w)
            .save_to_png(output.join(format!("selection-options-{theme:?}.png")))
            .unwrap();
    }
}

fn wait_tonal(d:&Driver) {
    let deadline=Instant::now()+Duration::from_secs(25);
    loop {pump(20);if d.w.gpu.borrow().as_ref().unwrap().session.require_document_idle().is_ok() {return;}
        assert!(Instant::now()<deadline,"tonal update timed out: {:?}",state(&d.w).host_error);}
}
#[test]
#[ignore = "isolated native-input.js --native-test=native_tonal_selection_input"]
fn native_tonal_selection_input() {
    let mut d=Driver::new("art.capycanvas.TonalSelection");
    let output=std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_|d.dir.to_string_lossy().into()));
    std::fs::create_dir_all(&output).unwrap();
    d.w.dispatch(UiAction::SetColor {rgba:[0.08,0.08,0.08,1.]});
    d.w.dispatch(UiAction::SetBrushOpacity {value:1.});
    for (a,b) in [([600.,600.],[850.,850.]),([1150.,600.],[1400.,850.])] {
        d.w.dispatch(UiAction::Invoke {command:CommandId::RectangleSelect});canvas_drag(&mut d,a,b,false);wait_selection(&d);
        d.w.dispatch(UiAction::Invoke {command:CommandId::FillSelection});pump(500);
        d.w.dispatch(UiAction::Invoke {command:CommandId::Deselect});
    }
    let opener=d.header_tool(ToolbarControl::Command {command:CommandId::Select});
    d.click_name(&opener);if state(&d.w).customization.drawer.is_none() {d.click_name(&opener);}
    d.click_name("tool-choice-TonalSelect");wait_tonal(&d);
    assert!(selection(&d).is_none(),"opening the tool does not change the selection");
    let panel=d.named("drawer-panel-ToolSettings");
    for absent in ["tool-list-tonal-source","tool-info-tonal-status","selection-actions-menu"] {
        assert!(find_named(&panel,absent).is_none(),"obsolete control: {absent}");
    }
    assert!(panel.width() >= layer_ui::TOOL_SETTINGS_MIN_WIDTH as i32);
    assert!(panel.measure(gtk::Orientation::Horizontal, -1).0 <= layer_ui::TOOL_SETTINGS_MIN_WIDTH as i32);
    let modes=d.named("selection-mode-row").compute_bounds(&panel).unwrap();
    let tones=d.named("tool-choice-tonal-tones-0").compute_bounds(&panel).unwrap();
    assert!(modes.y()<tones.y(),"selection mode comes first");
    let bar=d.named("tool-choice-bar-tonal-tones");
    assert!(bar.height()<=36,"all tones occupy one compact bar");
    let mut right=0.;
    for index in 0..6 {
        let button=d.named(&format!("tool-choice-tonal-tones-{index}"));
        assert!(button.is::<gtk::ToggleButton>());
        let bounds=button.compute_bounds(&bar).unwrap();
        assert_eq!(bounds.y(),0.,"tone choices must not wrap into rows");
        assert!(bounds.x()>=right && bounds.width()>=24.);
        right=bounds.x()+bounds.width();
        assert!(button.tooltip_text().unwrap().contains("stop"));
    }
    let p=d.point(&d.named("tool-choice-tonal-tones-0"));
    d.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));wait_tonal(&d);
    let first=wait_selection(&d);
    assert!(byte_pixel(&first,700,700)>240 && byte_pixel(&first,1250,700)>240);
    assert_eq!(byte_pixel(&first,1000,700),0);
    let image=d.w.gpu.borrow().as_ref().unwrap().session.engine().backend().capture().unwrap();
    let m=state(&d.w).camera.document_to_surface();
    let [x,y]=[(m[0]*1250.+m[2]*700.+m[4]) as usize,(m[1]*1250.+m[3]*700.+m[5]) as usize];
    let pixel=&image.bytes[y*image.stride as usize+x*4..][..4];
    assert!(pixel[0].abs_diff(pixel[1])<4 && pixel[1].abs_diff(pixel[2])<4,"ordinary selection has no tint: {pixel:?}");
    d.number(&d.named("tool-setting-tonal_softness"),"50");wait_tonal(&d);
    d.number(&d.named("tool-setting-selection_feather"),"2");wait_tonal(&d);
    let refined=selection(&d).unwrap();
    let settings_height=|d:&Driver| {
        let panel=d.named("drawer-panel-ToolSettings");
        let modes=d.named("selection-mode-row").compute_bounds(&panel).unwrap();
        let feather=d.named("tool-setting-selection_feather").compute_bounds(&panel).unwrap();
        feather.y()+feather.height()-modes.y()
    };
    let preset_height=settings_height(&d);
    assert!(preset_height<=150.,"preset controls use {preset_height}px");
    eprintln!("Tonal preset: controls {preset_height}px; drawer {}px",d.named("tool-drawer").height());
    assert_shared_icons(&d.named("tool-drawer"));
    let _=crate::snapshot(&d.w);pump(100);crate::snapshot(&d.w).save_to_png(output.join("tonal-presets.png")).unwrap();
    d.click_name(&opener);
    d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});pump(100);assert!(selection(&d).is_none(),"one undo includes slider refinements");
    d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});pump(100);assert_eq!(selection(&d),Some(refined.clone()));
    canvas_click(&mut d,[700.,700.]);wait_tonal(&d);
    let sampled=selection(&d).unwrap();assert!(byte_pixel(&sampled,1250,700)>240);
    d.click_name(&opener);
    assert_eq!(state(&d.w).tool_settings.len(),4,"Custom adds just two bounds");
    assert!(state(&d.w).tool_settings.iter().find(|f|f.id=="tonal_lower").unwrap().value < -5.);
    assert!(d.named("tool-choice-tonal-tones-5").downcast_ref::<gtk::ToggleButton>().unwrap().is_active());
    for id in ["tonal_lower", "tonal_upper"] {
        let number=d.named(&format!("tool-setting-{id}"));
        assert!(number.tooltip_text().unwrap().contains("stops relative to reference white"));
        let readout=find_css(&number,"number-readout").unwrap().downcast::<gtk::Label>().unwrap();
        assert!(!readout.text().contains("stop"),"units stay in label tooltips");
        assert_eq!(readout.text().split('.').last().unwrap().len(),1,"one decimal in the endpoint readout");
    }
    let custom_height=settings_height(&d);
    assert!(custom_height<=170.,"custom controls use {custom_height}px");
    let range=d.named("tool-range-tonal");
    assert!(range.height()<=28);
    assert!(d.named("drawer-panel-ToolSettings").measure(gtk::Orientation::Horizontal,-1).0<=layer_ui::TOOL_SETTINGS_MIN_WIDTH as i32);
    let low=d.named("tool-setting-tonal_lower").compute_bounds(&range).unwrap();
    let track=d.named("range-track-tonal").compute_bounds(&range).unwrap();
    let high=d.named("tool-setting-tonal_upper").compute_bounds(&range).unwrap();
    assert!(low.x()+low.width()<=track.x() && track.x()+track.width()<=high.x());
    assert!(low.width()<=48. && high.width()<=48.,"endpoint boxes fit the displayed numbers: {} / {}",low.width(),high.width());
    assert!(track.width()>=range.width() as f32*0.7,"track uses the space released by the endpoint boxes");
    eprintln!("Tonal range widths: low {}px, track {}px, high {}px",low.width(),track.width(),high.width());
    eprintln!("Tonal Custom: controls {custom_height}px; drawer {}px",d.named("tool-drawer").height());
    let _=crate::snapshot(&d.w);pump(100);crate::snapshot(&d.w).save_to_png(output.join("tonal-custom.png")).unwrap();
    d.key(b'q' as u32);wait_tonal(&d);
    assert!(state(&d.w).layer_tools.quick_mask);
    assert_eq!(selection(&d),Some(sampled.clone()));
    if state(&d.w).customization.drawer.is_none() {d.click_name(&opener);}
    let _=crate::snapshot(&d.w);pump(100);crate::snapshot(&d.w).save_to_png(output.join("tonal-quick-mask.png")).unwrap();
    let image=d.w.gpu.borrow().as_ref().unwrap().session.engine().backend().capture().unwrap();
    assert!(image.bytes.chunks_exact(4).filter(|p|p[0]>p[1].saturating_add(30)).count()>100);
    d.click_name(&opener);
    // The red overlay is excluded from both point and rectangle sampling.
    canvas_click(&mut d,[700.,700.]);wait_tonal(&d);
    assert!(byte_pixel(&selection(&d).unwrap(),1250,700)>240);
    assert!(state(&d.w).tool_settings.iter().find(|f|f.id=="tonal_lower").unwrap().value < -5.,"sampling ignores the red overlay");
    canvas_drag(&mut d,[930.,650.],[1050.,800.],false);wait_tonal(&d);
    let quick=selection(&d).unwrap();assert!(byte_pixel(&quick,1000,700)>240);assert_eq!(byte_pixel(&quick,700,700),0);
    d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});pump(100);
    assert_eq!(selection(&d),Some(sampled.clone()));
    d.key(b'q' as u32);
    d.w.dispatch(UiAction::Invoke {command:CommandId::NewSelectionLayer});wait_tonal(&d);
    let id=state(&d.w).layer_tools.mask_editing.unwrap().layer.unwrap();
    let saved=|d:&Driver| d.w.gpu.borrow().as_ref().unwrap().session.engine().document().saved_selection(layer_core::LayerId(id)).unwrap();
    let before=saved(&d);assert_eq!(before,layer_core::Selection::empty());
    d.click_name(&opener);d.click_name("tool-choice-tonal-tones-0");wait_tonal(&d);
    let mask=saved(&d);assert!(byte_pixel(&mask,700,700)>240 && byte_pixel(&mask,1250,700)>240);
    assert_eq!(byte_pixel(&mask,1000,700),0);assert_eq!(selection(&d),Some(sampled.clone()));
    let _=crate::snapshot(&d.w);pump(100);crate::snapshot(&d.w).save_to_png(output.join("tonal-selection-layer.png")).unwrap();
    d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});pump(100);assert_eq!(saved(&d),before);
    d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});pump(100);assert_eq!(saved(&d),mask);
    assert_eq!(selection(&d),Some(sampled));
    assert!(state(&d.w).host_error.is_none(),"{:?}",state(&d.w).host_error);
    d.w.window.destroy();pump(80);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_tonal_selection_pen_input --tablet"]
fn native_tonal_selection_pen_input() {
    let mut d=Driver::new("art.capycanvas.TonalSelectionPen");
    d.w.dispatch(UiAction::Invoke {command:CommandId::TonalSelect});
    d.key(b'q' as u32);assert!(state(&d.w).layer_tools.quick_mask);
    canvas_drag(&mut d,[600.,600.],[850.,800.],true);wait_tonal(&d);
    let first=wait_selection(&d);
    assert!(byte_pixel(&first,1500,1000)>240,"pen range selects matching tones throughout the image");
    d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});pump(100);assert!(selection(&d).is_none());
    d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});pump(100);assert_eq!(selection(&d),Some(first.clone()));
    d.w.dispatch(UiAction::Invoke {command:CommandId::Brush});assert_eq!(selection(&d),Some(first));
    d.w.window.destroy();pump(80);
}

#[test]
#[ignore = "private Mutter: --native-test=native_tonal_range_input"]
fn native_tonal_range_input() {
    tonal_range_input(false);
}
#[test]
#[ignore = "private Mutter: --native-test=native_tonal_range_pen_input --tablet"]
fn native_tonal_range_pen_input() {
    tonal_range_input(true);
}
fn tonal_range_input(pen: bool) {
    let mut d=Driver::new("art.capycanvas.TonalRange");
    d.w.dispatch(UiAction::Invoke {command:CommandId::SelectAll});
    let baseline=selection(&d);
    d.w.dispatch(UiAction::Invoke {command:CommandId::TonalSelect});
    let opener=d.header_tool(ToolbarControl::Command {command:CommandId::Select});
    d.click_name(&opener);if state(&d.w).customization.drawer.is_none() {d.click_name(&opener);}
    d.click_name("tool-choice-tonal-tones-5");wait_tonal(&d);
    let bounds=|d:&Driver| {
        ["tonal_lower","tonal_upper"].map(|id|state(&d.w).tool_settings.iter().find(|f|f.id==id).unwrap().value)
    };
    let handle_point=|d:&Driver,index:usize| {
        let name=["tonal_lower","tonal_upper"][index];
        let handle=d.named(&format!("range-handle-{name}")).downcast::<crate::range_control::RangeHandle>().unwrap();
        let mut p=d.point(handle.upcast_ref());
        p[0]+=handle.position(handle.value()) as f32-handle.width() as f32/2.+if index==0 {-4.} else {4.};p
    };
    let move_handle=|d:&mut Driver,index,device:&str,delta| {
        let a=handle_point(d,index);let b=[a[0]+delta,a[1]];
        let events=match device {
            "touch"=>serde_json::json!([{"touch":"down","point":a},{"touch":"move","point":b},{"touch":"up"}]),
            "pen"=>serde_json::json!([{"pen":"move","point":a},{"pen":"down"},{"pen":"move","point":b},{"pen":"up"},{"pen":"leave"}]),
            _=>serde_json::json!([{"point":a,"down":true},{"point":b},{"down":false}]),
        };
        d.perform(events);wait_tonal(d);
    };
    for device in if pen { &["pen"][..] } else { &["mouse","touch"][..] } {
        let before=bounds(&d);
        move_handle(&mut d,0,device,-8.);
        let low=bounds(&d);assert!(low[0]<before[0],"{device}: lower handle moves");assert_eq!(low[1],before[1]);
        move_handle(&mut d,1,device,8.);
        let high=bounds(&d);assert!(high[1]>low[1],"{device}: upper handle moves");assert_eq!(high[0],low[0]);
    }
    // The synthetic tablet proxy is for contacts. Numeric text focus runs
    // through the compositor directly in the mouse/touch journey.
    if pen { assert!(state(&d.w).host_error.is_none()); d.finish(); return; }
    // Keyboard continues on the focused native handle.
    let before=bounds(&d);d.key(0xff53);wait_tonal(&d);
    assert!(bounds(&d)[1]>before[1]);
    // As with other focused GtkRanges, ordinary shortcuts belong to the
    // control. Exercise the mask action without changing native key ownership.
    d.w.dispatch(UiAction::Invoke {command:CommandId::QuickMask});wait_tonal(&d);
    assert!(state(&d.w).layer_tools.quick_mask);
    let before=bounds(&d);move_handle(&mut d,0,"mouse",-5.);
    assert!(bounds(&d)[0]<before[0],"range remains live when Quick Mask changes the tool context");
    // Escape cancels the current drag and returns just that endpoint.
    let before=bounds(&d);let a=handle_point(&d,0);let b=[a[0]+15.,a[1]];
    d.perform(serde_json::json!([{"point":a,"down":true},{"point":b}]));pump(200);
    assert_ne!(bounds(&d),before);
    d.key(0xff1b);d.perform(serde_json::json!([{"down":false}]));wait_tonal(&d);
    assert_eq!(bounds(&d),before);
    if state(&d.w).customization.drawer.is_none() { d.click_name(&opener); }
    let lower=d.named("tool-setting-tonal_lower");let upper=d.named("tool-setting-tonal_upper");
    d.number(&lower,"-20");wait_tonal(&d);d.number(&upper,"12");wait_tonal(&d);
    assert_eq!(bounds(&d),[-20.,12.],"typed values extend beyond the normal track domain");
    d.number(&lower,"13");wait_tonal(&d);assert_eq!(bounds(&d),[12.,12.]);
    move_handle(&mut d,0,"mouse",-12.);assert!(bounds(&d)[0]<12.,"coincident handles separate again");
    let result=selection(&d);
    d.click_name(&opener);d.w.dispatch(UiAction::Invoke {command:CommandId::Undo});pump(100);assert_eq!(selection(&d),baseline);
    d.w.dispatch(UiAction::Invoke {command:CommandId::Redo});pump(100);assert_eq!(selection(&d),result);
    assert!(state(&d.w).host_error.is_none(),"{:?}",state(&d.w).host_error);
    d.finish();
}
