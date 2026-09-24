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
        assert!(button.compute_bounds(&tools).unwrap().height() >= 44., "touchable selection tool row, including padding");
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
        assert_eq!(state(&d.w).tool_panels.tools.subtools.len(), 7);
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
        d.header_icon(CommandId::Select, "color-select");
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
    for tool in SelectionTool::ALL.into_iter().filter(|t| *t != SelectionTool::Brush) {
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
