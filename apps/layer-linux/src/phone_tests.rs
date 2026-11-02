use super::*;
use super::workspace_header::Driver;
use layer_render::CanvasRenderer;

fn setup(name: &str) -> Driver {
    let d = super::phone_motion::setup(name);
    if let Ok(viewport) = std::env::var("LAYER_MOTION_VIEWPORT") {
        let (width, height) = viewport.split_once('x').unwrap();
        let expected = [width.parse::<i32>().unwrap(), height.parse::<i32>().unwrap()];
        let actual = [d.w.surface.width(), d.w.surface.height()];
        assert_eq!(actual, expected, "the journey must use its requested phone viewport");
    }
    if d.w.surface.width().min(d.w.surface.height()) < 480 {
        assert!(presentation(&d).tile > 0.);
        assert_eq!(state(&d.w).workspace.layout.phone.as_ref().unwrap().iter().map(|tile| tile.control).collect::<Vec<_>>(),
            WorkspacePreset::Painter.phone_controls());
    }
    d
}

fn presentation(d: &Driver) -> layer_ui::PhoneLayout {
    state(&d.w).phone_presentation([d.w.surface.width() as f32, d.w.surface.height() as f32]).unwrap()
}

fn click(d: &mut Driver, name: &str) {
    let widget = widgets(d.w.window.upcast_ref()).find(|widget| widget.is_mapped() && widget.widget_name() == name)
        .unwrap_or_else(|| panic!("missing mapped {name}"));
    if let Some(scroll) = std::iter::successors(widget.parent(), |parent| parent.parent())
        .find_map(|parent| parent.downcast::<gtk::ScrolledWindow>().ok()) {
        if widget.is_focusable() {
            assert!(widget.grab_focus());
            until(|| widget.compute_bounds(&scroll).is_some_and(|bounds|
                bounds.y() >= -1. && bounds.y() + bounds.height() <= scroll.height() as f32 + 1.),
                "scroll the actual menu action into view before clicking");
        }
    }
    d.click(&widget);
}

fn capture_current(d: &mut Driver, name: &str) {
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        let theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "light".into());
        d.input.perform(serde_json::json!([{ "capture_window": {
            "name": format!("phone-{}x{}-{theme}-{name}", d.w.surface.width(), d.w.surface.height()),
            "maximized": d.w.window.is_maximized(),
        } }]));
    }
}

pub(super) fn capture(d: &mut Driver, name: &str) {
    let capy = d.named("zen-capy");
    let tooltip = capy.has_tooltip();
    capy.set_has_tooltip(false);
    clear_canvas_pointer(d);
    capy.set_has_tooltip(false);
    capy.trigger_tooltip_query();
    capture_current(d, name);
    capy.set_has_tooltip(tooltip);
}

pub(super) fn clear_canvas_pointer(d: &mut Driver) {
    let capy = d.named("zen-capy");
    d.input.perform(serde_json::json!([{ "point": d.point(&capy) }]));
    until(|| ui_session_mut(&d.w).canvas_cursor().is_none(), "pointer leaves the canvas before capture");
    until(|| ui_session(&d.w).engine().backend().frames_idle(), "capture uses the published canvas frame");
}

fn tool(d: &Driver, control: ToolbarControl) -> String {
    let id = state(&d.w).workspace.layout.phone.as_ref().unwrap().iter().find(|tile| tile.control == control).unwrap().id;
    format!("phone-tool-{id}")
}

fn inside(d: &Driver, widget: &gtk::Widget) {
    assert!(widget.is_mapped(), "{} is mapped", widget.widget_name());
    let bounds = widget.compute_bounds(&d.w.surface).unwrap();
    assert!(bounds.width() > 0. && bounds.height() > 0.);
    assert!(bounds.x() >= -1. && bounds.y() >= -1.
        && bounds.x() + bounds.width() <= d.w.surface.width() as f32 + 1.
        && bounds.y() + bounds.height() <= d.w.surface.height() as f32 + 1.,
        "{} stays inside the viewport: {bounds:?}", widget.widget_name());
}

fn frame(d: &Driver) {
    let layout = state(&d.w).workspace.layout;
    for tile in layout.phone.as_ref().unwrap().iter().filter(|tile| tile.control != ToolbarControl::Divider) {
        inside(d, &d.named(&format!("phone-tool-{}", tile.id)));
    }
    for name in ["zen-capy", "component-slider-size", "component-slider-strength"] { inside(d, &d.named(name)); }
    let phone = presentation(d);
    let first = state(&d.w).workspace.layout.phone.unwrap().into_iter().find(|tile| tile.control != ToolbarControl::Divider).unwrap();
    let button = d.named(&format!("phone-tool-{}", first.id));
    let bounds = button.compute_bounds(&d.w.surface).unwrap();
    let picked = d.w.surface.pick((bounds.x()+bounds.width()*0.5) as f64,
        (bounds.y()+bounds.height()*0.5) as f64, gtk::PickFlags::DEFAULT).unwrap();
    assert!(picked == button || picked.is_ancestor(&button), "phone controls receive native input");
    for (name, expected) in [("phone-tools-chip", phone.chips[0]), ("phone-sliders-chip", phone.chips[1]),
        ("phone-bar-tools", phone.bars[0]), ("phone-bar-capy", phone.bars[1])] {
        let widget = d.named(name);
        inside(d, &widget);
        let bounds = widget.compute_bounds(&d.w.surface).unwrap();
        assert!([bounds.x()-expected.x,bounds.y()-expected.y,bounds.width()-expected.width,bounds.height()-expected.height]
            .into_iter().all(|difference| difference.abs() <= 1.), "{name} uses shared continuous geometry");
    }
    for (index, chip) in [(0, phone.chips[0]), (1, phone.chips[1])] {
        let parts = if index == 0 { phone.tools.to_vec() } else { vec![phone.size, phone.strength] };
        for part in parts.into_iter().filter(|part| part.width > 0. && part.height > 0.) {
            assert!(part.x >= chip.x-0.01 && part.y >= chip.y-0.01 && part.x + part.width <= chip.x + chip.width + 0.01
                && part.y + part.height <= chip.y + chip.height + 0.01,
                "phone control belongs to its continuous chip: part={part:?}, chip={chip:?}");
        }
    }
    let gap = if d.w.surface.width() <= d.w.surface.height() {
        if phone.size.x < phone.strength.x { phone.strength.x-phone.size.x-phone.size.width }
        else { phone.size.x-phone.strength.x-phone.strength.width }
    }
        else { phone.strength.y-phone.size.y-phone.size.height };
    assert!((gap-2.).abs() <= 0.01, "sliders share one chip with the internal gap");
    for button in widgets(d.w.phone.root.upcast_ref()).filter(|widget| widget.is_mapped() && widget.is::<gtk::Button>()
        && widget.has_css_class("close")) {
        let bounds = button.compute_bounds(&d.w.surface).unwrap();
        let center = [bounds.x() + bounds.width() * 0.5, bounds.y() + bounds.height() * 0.5];
        let bar = phone.bars.iter().find(|bar| center[0] >= bar.x && center[0] <= bar.x + bar.width
            && center[1] >= bar.y && center[1] <= bar.y + bar.height).unwrap();
        let difference = if d.w.surface.width() <= d.w.surface.height() {
            center[1] - bar.y - bar.height * 0.5
        } else { center[0] - bar.x - bar.width * 0.5 };
        assert!(difference.abs() <= 1., "native close button shares its bar centerline: {bounds:?}, {bar:?}");
    }
    let counts = state(&d.w).phone.native_counts;
    let native_buttons = widgets(d.w.phone.root.upcast_ref()).filter(|widget| widget.is_mapped() && widget.is::<gtk::Button>()
        && std::iter::successors(widget.parent(), |parent| parent.parent()).any(|parent| parent.is::<gtk::WindowControls>())).count();
    assert_eq!(native_buttons, counts.iter().sum::<u32>() as usize, "every live native window control is mapped");
    for (side, count) in counts.into_iter().enumerate() {
        if d.w.surface.width() <= d.w.surface.height() {
            if count > 0 { inside(d, &d.named(&format!("native-group-{side}"))); }
        } else {
            let controls = widgets(d.w.phone.root.upcast_ref()).filter_map(|widget| widget.downcast::<gtk::WindowControls>().ok())
                .filter(|control| control.widget_name().starts_with(&format!("native-control-{side}-")) && !control.is_empty()).collect::<Vec<_>>();
            assert_eq!(controls.len(),count as usize);
            for (index, control) in controls.iter().enumerate() {
                inside(d,control.upcast_ref());
                let actual = control.compute_bounds(&d.w.surface).unwrap();
                let expected = phone.native_control(side,index as u32).unwrap();
                assert!([actual.x()-expected.x,actual.y()-expected.y,actual.width()-expected.width,actual.height()-expected.height]
                    .into_iter().all(|difference| difference.abs() <= 1.),"live native control uses its compacted slot: {actual:?}, {expected:?}");
            }
        }
    }
}

fn menu(d: &mut Driver) {
    click(d, "zen-capy");
    until(|| state(&d.w).phone.menu, "Capy's menu opens");
    let menu = d.named("phone-menu");
    until(|| d.w.phone.placement.borrow().is_some(), "phone menu placement is published");
    let swapped = state(&d.w).phone.swapped;
    capture(d, &format!("menu-open-{swapped}"));
    inside(d, &menu);
    let scrollers = [0, 1].map(|index| d.named(&format!("phone-menu-scroll-{index}")).downcast::<gtk::ScrolledWindow>().unwrap());
    for scroll in &scrollers { inside(d, scroll.upcast_ref()); }
    let columns = [0, 1].map(|index| d.named(&format!("phone-menu-column-{index}")));
    let placement = d.w.phone.placement.borrow().as_ref().unwrap().clone();
    assert_eq!(placement.columns.len(), 2);
    let [first, second] = [placement.columns[0], placement.columns[1]];
    let measurements = columns.each_ref().map(|column| column.measure(gtk::Orientation::Horizontal, -1));
    let widths = presentation(d).menu_column_widths(measurements.map(|m| m.0 as f32), measurements.map(|m| m.1 as f32));
    if d.w.surface.width() <= d.w.surface.height() {
        assert!(second.y >= first.y + first.height, "portrait menu columns stack");
        assert!((placement.bounds.width - presentation(d).bars[1].width).abs() <= 1.);
    } else if widths.iter().sum::<f32>() + PhoneLayout::MENU_GAP <= presentation(d).work_area.width {
        assert!((first.y - second.y).abs() <= 1. && second.x >= first.x + first.width,
            "landscape menu columns sit side by side when their minimum widths fit");
    }
    let drawing = d.named("phone-drawing");
    let visible = |widget: &gtk::Widget, scroll: &gtk::ScrolledWindow| widget.compute_bounds(scroll).is_some_and(|bounds|
        bounds.x() >= -1. && bounds.x() + bounds.width() <= scroll.width() as f32 + 1.
            && bounds.y() >= -1. && bounds.y() + bounds.height() <= scroll.height() as f32 + 1.);
    assert!(visible(&drawing, &scrollers[0]), "the drawing row is initially visible");
    assert!(visible(d.w.workspaces.switcher.upcast_ref(), &scrollers[0]), "the workspace switcher is initially visible");
    if d.w.surface.width() > d.w.surface.height() {
        let span = presentation(d).bars[1];
        let bounds = menu.compute_bounds(&d.w.surface).unwrap();
        assert!(bounds.y() >= span.y - 1. && bounds.y() + bounds.height() <= span.y + span.height + 1.,
            "the whole landscape menu stays in the Capy bar's span");
    }
    assert_eq!(d.named("phone-swap").property::<bool>("active"), state(&d.w).phone.swapped);
    assert_eq!(d.named("phone-zen").property::<bool>("active"), state(&d.w).phone.hidden);
    let swap = d.named("phone-swap");
    let check = widgets(&swap).find(|widget| widget.css_name()=="check").unwrap();
    assert_eq!(check.type_().name(),"GtkBuiltinIcon");
    assert_eq!(check.state_flags().contains(gtk::StateFlags::CHECKED),state(&d.w).phone.swapped);
    if state(&d.w).phone.swapped {
        inside(d,&check);
        let label = widgets(&swap).filter_map(|widget| widget.downcast::<gtk::Label>().ok())
            .find(|label| label.text()==swap.property::<String>("text")).unwrap();
        let marker = check.compute_bounds(&swap).unwrap();
        let text = label.compute_bounds(&swap).unwrap();
        assert!(if swap.direction()==gtk::TextDirection::Rtl {marker.x() >= text.x()+text.width()-1.}
            else {marker.x()+marker.width() <= text.x()+1.}, "native checked glyph stays in the leading space");
    }
    let models = ui_session(&d.w).phone_menu_columns();
    for (index, model) in models.iter().enumerate() {
        let column = &columns[index];
        let mut previous = -1.;
        let mut last = None;
        for item in model.sections.iter().flatten() {
            let label = widgets(column).filter_map(|widget| widget.downcast::<gtk::Label>().ok())
                .find(|label| label.is_mapped() && label.text() == item.label).unwrap();
            let bounds = label.compute_bounds(column).unwrap();
            let text_start = bounds.x()+label.layout_offsets().0 as f32;
            assert!((text_start-18.).abs() <= 1., "menu column {index} uses the desktop text start: {} at {text_start}", item.label);
            let y = bounds.y();
            assert!(y >= previous, "menu column {index} retains shared row order");
            previous = y;
            last = Some(label.clone().upcast::<gtk::Widget>());
            assert!(std::iter::successors(Some(label.clone().upcast::<gtk::Widget>()), |widget| widget.parent())
                .any(|widget| widget.type_().name() == "GtkModelButton"), "menu uses native rows: {}", item.label);
        }
        let mut last = last.unwrap();
        while !last.is_focusable() { last = last.parent().unwrap(); }
        assert!(last.grab_focus());
        until(|| visible(&last, &scrollers[index]), "menu column scroll reaches its last action");
    }
    assert!(d.named("zen-capy").grab_focus());
    for scroll in &scrollers { scroll.vadjustment().set_value(0.); }
    until(|| visible(&drawing, &scrollers[0]), "menu capture returns to its drawing row");
    menu_separator(d,"main");
    let file_label = ApplicationMenu::File.localized_label(&d.w.localization());
    let file = widgets(&columns[1]).find(|widget| widget.is_mapped() && widget.type_().name()=="GtkModelButton"
        && widget.property::<String>("text")==file_label.as_ref()).unwrap();
    let ranges_fit = scrollers.iter().all(|scroll| scroll.vadjustment().upper() <= scroll.vadjustment().page_size()+0.5);
    if visible(&swap,&scrollers[0]) && visible(&file,&scrollers[1]) {
        let a = swap.compute_bounds(&menu).unwrap();
        let separator = d.named("phone-menu-separator").downcast::<gtk::Separator>().unwrap();
        let b = separator.compute_bounds(&menu).unwrap();
        let c = file.compute_bounds(&menu).unwrap();
        let gaps = if separator.orientation()==gtk::Orientation::Horizontal {
            [b.y()-a.y()-a.height(),c.y()-b.y()-b.height()]
        } else { [b.x()-a.x()-a.width(),c.x()-b.x()-b.width()] };
        assert!(gaps.into_iter().all(|gap| (gap-6.).abs() <= 1.),
            "native section boundary keeps 6+1+6 spacing: {gaps:?}; Swap={a:?}, separator={b:?}, File={c:?}");
    } else { assert!(!ranges_fit,"boundary rows stay visible when both sections fit"); }
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_frame --native-storage; phone viewport and theme supplied externally"]
fn native_phone_frame() {
    let mut d = setup("art.capycanvas.PhoneFrame");
    if d.w.surface.width().min(d.w.surface.height()) >= 480 {
        capture(&mut d, "desktop-titlebar");
        let caption = ui_session(&d.w).command(CommandId::ZenMode).tooltip;
        assert_eq!(d.w.zen_capy.tooltip_text().as_deref(), Some(caption.as_str()));
        let desktop = [d.w.surface.width(), d.w.surface.height()];
        d.w.window.unmaximize();
        d.w.window.set_default_size(412, 891);
        until(|| [d.w.surface.width(), d.w.surface.height()] == [412, 891]
            && d.w.phone.root.is_mapped(), "desktop window enters phone layout");
        let phone_caption = ui_session(&d.w).capy_caption([412., 891.]);
        assert_eq!(d.w.zen_capy.tooltip_text().as_deref(), Some(phone_caption.as_ref()));
        for (control, value) in [(ToolbarControl::BrushSizeSlider, 43.), (ToolbarControl::BrushStrengthSlider, 0.37)] {
            d.w.dispatch(control.slider().unwrap().action(value));
        }
        d.w.window.maximize();
        until(|| [d.w.surface.width(), d.w.surface.height()] == desktop
            && d.w.header.root.is_mapped(), "phone window returns to desktop layout");
        assert_eq!(d.w.zen_capy.tooltip_text().as_deref(), Some(caption.as_str()));
        assert!(!gtk::test_accessible_has_property(&d.w.zen_capy, gtk::AccessibleProperty::Label));
        for control in PhoneLayout::SLIDERS {
            let id = state(&d.w).workspace.layout.panels.iter().flat_map(|panel| panel.tiles())
                .find(|tile| tile.control == control).unwrap().id;
            let slider = d.named(&format!("component-slider-{id}")).downcast::<gtk::Scale>().unwrap();
            let field = state(&d.w).toolbar_component(control).unwrap().numeric.unwrap();
            let expected = field.numeric.resolve(field.value as f64, NumericOperation::Format).unwrap().fill;
            assert!((slider.value() - expected).abs() < 1e-6, "desktop slider refreshes after phone edits: {control:?}");
        }
        desktop_menus(&mut d);
        d.finish();
        return;
    }
    let original = layer_ui::durable_layout(&state(&d.w).workspace.layout);
    let saved_zen = state(&d.w).workspace.zen_mode;
    frame(&d);
    capture(&mut d, "rest");
    mouse_paint(&mut d, "normal");
    let before = presentation(&d).capy;
    menu(&mut d);
    capture(&mut d, "menu");
    menu_pages(&mut d);
    click(&mut d, "phone-swap");
    until(|| state(&d.w).phone.swapped, "Swap Sides mirrors the bars");
    assert_ne!(presentation(&d).capy, before);
    frame(&d);
    capture(&mut d, "swapped");
    menu(&mut d);
    capture(&mut d, "menu-swapped");
    menu_pages(&mut d);
    click(&mut d, "phone-zen");
    until(|| state(&d.w).phone.hidden, "Zen hides the phone bars");
    assert!(!d.named("component-slider-size").is_mapped());
    assert!(!d.named(&tool(&d, ToolbarControl::Color)).is_mapped());
    for child in std::iter::successors(d.w.phone.root.first_child(), |child| child.next_sibling()) {
        assert!(!child.is_mapped(), "hidden phone child {} is unmapped", child.widget_name());
    }
    assert!(d.w.phone.placement.borrow().is_none());
    inside(&d, &d.named("zen-capy"));
    capture(&mut d, "hidden");
    mouse_paint(&mut d, "hidden");
    click(&mut d, "zen-capy");
    until(|| !state(&d.w).phone.hidden, "Capy restores the bars");
    frame(&d);
    mouse_paint(&mut d, "restored");
    d.input.key(0xff09);
    until(|| state(&d.w).phone.hidden, "Tab hides the bars");
    d.input.key(0xff1b);
    until(|| !state(&d.w).phone.hidden, "Escape restores the bars");
    assert_eq!(layer_ui::durable_layout(&state(&d.w).workspace.layout), original);
    assert_eq!(state(&d.w).workspace.zen_mode, saved_zen);
    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
        layout: WorkspacePreset::Illustrator.layout(Platform::Gtk), ..Default::default()
    }) });
    d.w.dispatch(UiAction::Invoke { command: CommandId::ActualPixels });
    new_photo::ready(&d.w);
    frame(&d);
    capture(&mut d, "paint-default");
    for (name, control) in [("eraser", ToolbarControl::Command { command: CommandId::Eraser }),
        ("fill", ToolbarControl::ToolSlot { slot: ToolSlotId::Fill })] {
        let tile = tool(&d, control);
        click(&mut d, &tile);
        let view = state(&d.w).phone_tools().into_iter().find(|view| format!("phone-tool-{}",view.id) == tile).unwrap();
        let ToolbarControl::Command { command } = view.resolved_control else { panic!("selected phone tool resolves to a command") };
        assert!(ui_session(&d.w).command(command).selected);
        capture(&mut d, &format!("paint-{name}-selected"));
    }
    d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    new_photo::ready(&d.w);
    for device in ["mouse", "touch"] {
        let swapped = state(&d.w).phone.swapped;
        let point = d.point(&d.named("zen-capy"));
        d.input.perform(serde_json::json!([contact(device, "down", point), {"wait_ms": 900}]));
        assert!(state(&d.w).phone.capy_drag, "{device} holds Capy");
        d.input.perform(serde_json::json!([contact(device, "up", point)]));
        assert!(!state(&d.w).phone.menu, "{device} hold does not click Capy");
        assert!(!state(&d.w).phone.capy_drag);
        assert_eq!(state(&d.w).phone.swapped, swapped, "hold without crossing keeps sides");
        let point = d.point(&d.named("zen-capy"));
        let toolbar = presentation(&d).bars[0];
        let target = screen_point(d.w.surface.upcast_ref(), &d.w.window,
            [(toolbar.x + toolbar.width / 2.) / d.w.surface.width() as f32,
             (toolbar.y + toolbar.height / 2.) / d.w.surface.height() as f32]);
        d.input.perform(serde_json::json!([contact(device, "down", point), {"wait_ms": 900},
            contact(device, "move", target), contact(device, "up", target)]));
        assert_eq!(state(&d.w).phone.swapped, !swapped, "{device} held Capy drop swaps sides");
        assert!(!state(&d.w).phone.menu);
        assert!(!state(&d.w).phone.capy_drag);
        frame(&d);
    }
    d.finish();
}

fn desktop_menus(d: &mut Driver) {
    for (preset, custom) in [(WorkspacePreset::Painter, false), (WorkspacePreset::Illustrator, false),
        (WorkspacePreset::Photographer, false), (WorkspacePreset::Painter, true)] {
        let mut layout = preset.layout(Platform::Gtk);
        if preset == WorkspacePreset::Illustrator {
            let controls = layout.panels.iter().find(|panel| PhoneLayout::SLIDERS.into_iter()
                .all(|control| panel.tiles().iter().any(|tile| tile.control == control))).unwrap().id;
            layout.set_panel_visible(controls, true).unwrap();
            layout.move_panel([d.w.surface.width() as f32,d.w.surface.height() as f32], controls,
                DockTarget::Edge { edge: Edge::Bottom, outer: false }).unwrap();
        }
        let remove = layout.header.entries().filter(|entry| entry.item == HeaderItem::MenuLabels
            || custom && matches!(entry.item, HeaderItem::DocumentTitle | HeaderItem::Workspaces))
            .map(|entry| entry.id).collect::<Vec<_>>();
        for id in remove { layout.header.remove(id).unwrap(); }
        if !layout.header.entries().any(|entry| entry.item == HeaderItem::Menu) {
            layout.header.add(HeaderZone::Left, None, &[HeaderItem::Menu]).unwrap();
        }
        let id = layout.header.entries().find(|entry| entry.item == HeaderItem::Menu).unwrap().id;
        d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState { layout, ..Default::default() }) });
        pump(250);
        if preset == WorkspacePreset::Illustrator {
            capture(d, "desktop-brush-controls");
            let id = state(&d.w).workspace.layout.panels.iter().flat_map(|panel| panel.tiles())
                .find(|tile| tile.control == ToolbarControl::BrushSizeSlider).unwrap().id;
            let slider = d.named(&format!("component-slider-{id}"));
            let point = d.point(&slider);
            d.input.perform(serde_json::json!([contact("mouse","down",point)]));
            assert!(d.named("brush-slider-preview").is_mapped());
            capture_current(d,"desktop-brush-controls-preview");
            d.input.perform(serde_json::json!([contact("mouse","up",point)]));
            d.input.key(0xff1b);
        }
        let expected = ui_session(&d.w).main_menu_extras();
        let root = d.named(&format!("header-item-{id}"));
        let button = root.clone().downcast::<gtk::MenuButton>().ok().or_else(|| descendant::<gtk::MenuButton>(&root)).unwrap();
        d.click(button.upcast_ref());
        let popup = button.popover().unwrap().downcast::<gtk::PopoverMenu>().unwrap();
        until(|| popup.is_mapped(), "desktop Main Menu opens");
        let body = find_named(popup.upcast_ref(), "capy-main-extra");
        let drawing = body.as_ref().and_then(|body| body.first_child()).is_some_and(|widget| widget.is::<gtk::Button>());
        let switcher = body.as_ref().is_some_and(|body| d.w.workspaces.switcher.parent().as_ref() == Some(body));
        assert_eq!([drawing, switcher], expected, "{preset:?}/{custom}: native Main Menu extras");
        assert_eq!(body.as_ref().is_some_and(|body| body.is_mapped()), expected.into_iter().any(|value| value));
        capture(d, &format!("desktop-main-menu-{preset:?}-{custom}").to_lowercase());
        d.input.key(0xff1b);
        until(|| !popup.is_mapped(), "Escape closes desktop Main Menu");
    }
}

fn scroll_overflow(d: &mut Driver) {
    let scrollers = widgets(d.w.window.upcast_ref()).filter_map(|widget| widget.downcast::<gtk::ScrolledWindow>().ok())
        .filter(|scroll| scroll.is_mapped()).collect::<Vec<_>>();
    for scroll in scrollers {
        let bounds = scroll.compute_bounds(&d.w.surface).unwrap();
        let mut clip = [bounds.x().max(0.), bounds.y().max(0.),
            (bounds.x() + bounds.width()).min(d.w.surface.width() as f32),
            (bounds.y() + bounds.height()).min(d.w.surface.height() as f32)];
        for parent in std::iter::successors(scroll.parent(), |parent| parent.parent()) {
            if parent.overflow() == gtk::Overflow::Hidden || parent.is::<gtk::Viewport>() || parent.is::<gtk::ScrolledWindow>() {
                if let Some(bounds) = parent.compute_bounds(&d.w.surface) {
                    clip = [clip[0].max(bounds.x()), clip[1].max(bounds.y()),
                        clip[2].min(bounds.x() + bounds.width()), clip[3].min(bounds.y() + bounds.height())];
                }
            }
        }
        if clip[2] - clip[0] <= 2. || clip[3] - clip[1] <= 2. { continue; }
        for (axis, adjustment, policy) in [(0, scroll.vadjustment(), scroll.vscrollbar_policy()),
            (1, scroll.hadjustment(), scroll.hscrollbar_policy())] {
            if !matches!(policy, gtk::PolicyType::Automatic | gtk::PolicyType::Always)
                || adjustment.upper() - adjustment.page_size() <= 1. { continue; }
            let before = adjustment.value();
            let target = scroll.upcast_ref::<gtk::Widget>();
            let Some((point, picked)) = [[0.5, 0.5], [0.1, 0.1], [0.9, 0.1], [0.1, 0.9], [0.9, 0.9]].into_iter()
                .find_map(|fraction| {
                    let point = [clip[0] + (clip[2] - clip[0]) * fraction[0], clip[1] + (clip[3] - clip[1]) * fraction[1]];
                    d.w.surface.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT)
                        .filter(|picked| picked.ancestor(gtk::ScrolledWindow::static_type()).as_ref() == Some(target)
                            && picked.ancestor(gtk::Scale::static_type()).is_none()
                            && picked.ancestor(crate::number_control::NumberControl::static_type()).is_none())
                        .map(|picked| (point, picked))
                }) else { continue; };
            let path = std::iter::successors(Some(picked), |widget| widget.parent())
                .map(|widget| format!("{}:{}", widget.type_().name(), widget.widget_name())).collect::<Vec<_>>();
            let diagnostic = format!("wheel axis={axis} policy={policy:?} scroller={} clip={clip:?} before={before} upper={} page={} picked={path:?}",
                scroll.widget_name(), adjustment.upper(), adjustment.page_size());
            let point = screen_point(d.w.surface.upcast_ref(), &d.w.window,
                [point[0] / d.w.surface.width() as f32, point[1] / d.w.surface.height() as f32]);
            let direction = if before < adjustment.upper() - adjustment.page_size() { 1 } else { -1 };
            d.input.perform(serde_json::json!([{"point": point}, {"wheel": [axis, direction]}]));
            until(|| (adjustment.value() - before).abs() > 0.5, &diagnostic);
            adjustment.set_value(before);
        }
    }
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_drawers --native-storage; phone viewport and theme supplied externally"]
fn native_phone_drawers() {
    let mut d = setup("art.capycanvas.PhoneDrawers");
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    for swapped in [false, true] {
        if swapped { menu(&mut d); click(&mut d, "phone-swap"); }
        for (name, control, panel) in [
            ("brush", ToolbarControl::Command { command: CommandId::DrawingBrush }, Panel::BrushSets),
            ("color", ToolbarControl::Color, Panel::Color),
            ("layers", ToolbarControl::Panel { panel: Panel::Layers }, Panel::Layers),
        ] {
            let opener = tool(&d, control);
            let selected = match control { ToolbarControl::Command { command } => ui_session(&d.w).command(command).selected, _ => true };
            click(&mut d, &opener);
            if !selected { click(&mut d, &opener); }
            until(|| state(&d.w).customization.drawer.is_some(), "phone tool opens its drawer");
            until(|| d.w.drawer.placement().is_some_and(|place| {
                let width = place.columns.iter().map(|column| column.x + column.width).fold(0., f32::max);
                let height = place.columns.iter().map(|column| column.y + column.height).fold(0., f32::max);
                (place.bounds.width - width).abs() < 0.01 && (place.bounds.height - height).abs() < 0.01
            }), "drawer opening reaches its final column geometry");
            let drawer = state(&d.w).customization.drawer.unwrap();
            assert!(drawer.columns.iter().flatten().any(|shown| *shown == panel));
            let place = d.w.drawer.placement().unwrap();
            let source = d.named(&opener).compute_bounds(&d.w.surface).unwrap();
            let source_edge = match place.direction {
                Edge::Top => source.y(), Edge::Bottom => source.y() + source.height(),
                Edge::Left => source.x(), Edge::Right => source.x() + source.width(),
            };
            let anchor_edge = match place.direction {
                Edge::Top => place.anchor.y, Edge::Bottom => place.anchor.y + place.anchor.height,
                Edge::Left => place.anchor.x, Edge::Right => place.anchor.x + place.anchor.width,
            };
            assert!((source_edge - anchor_edge).abs() <= 0.5,
                "{name}/{swapped}: drawer bridge starts on opener edge: {source_edge} vs {anchor_edge}");
            assert!(place.connection().is_some(), "phone drawer has its visible bridge");
            let viewport = [d.w.surface.width() as f32, d.w.surface.height() as f32];
            assert!(place.bounds.x >= 0. && place.bounds.y >= 0.
                && place.bounds.x + place.bounds.width <= viewport[0] + 1.
                && place.bounds.y + place.bounds.height <= viewport[1] + 1.);
            if viewport[0] <= viewport[1] {
                let toolbar = presentation(&d).bars[0];
                assert!((place.bounds.x - toolbar.x).abs() <= 1. && (place.bounds.width - toolbar.width).abs() <= 1.,
                    "portrait {name} drawer spans the toolbar");
            } else if panel == Panel::Layers {
                assert!((place.bounds.width - 280.).abs() <= 1., "landscape Layers retains its width");
            }
            let content = d.named(&format!("drawer-panel-{panel:?}"));
            let viewport = content.ancestor(gtk::ScrolledWindow::static_type()).unwrap_or(content);
            inside(&d, &viewport);
            if panel == Panel::Color {
                let wheel = d.named("color-wheel").downcast::<crate::tool_panels::ColorWheel>().unwrap();
                let content = d.named("drawer-panel-Color");
                assert_eq!(wheel.width(), content.width(), "color wheel fills its content column");
                let expected = layer_ui::ColorPanelLayout::new(content.width() as f32).unwrap().wheel[2];
                assert!((wheel.drawing_bounds().0 - expected).abs() <= 1.,
                    "phone wheel uses the docked Color panel proportions");
                let palettes = d.named("drawer-panel-Palettes");
                inside(&d, &palettes);
                for scroll in widgets(&palettes).filter_map(|widget| widget.downcast::<gtk::ScrolledWindow>().ok())
                    .filter(|scroll| scroll.is_mapped()) {
                    let adjustment = scroll.hadjustment();
                    assert!(adjustment.upper() <= adjustment.page_size() + 0.5,
                        "palette has no horizontal overflow: {} > {}", adjustment.upper(), adjustment.page_size());
                }
                for grid in widgets(&palettes).filter_map(|widget| widget.downcast::<crate::palette_grid::PaletteGrid>().ok()) {
                    for child in std::iter::successors(grid.first_child(), |child| child.next_sibling()).filter(|child| child.is_mapped()) {
                        let bounds = grid.visual_bounds(&child).unwrap();
                        assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= grid.width() as f32,
                            "palette cell stays inside its grid: {bounds:?}");
                    }
                }
            }
            scroll_overflow(&mut d);
            capture(&mut d, &format!("{name}-{}", if swapped { "swapped" } else { "normal" }));
            d.input.key(0xff1b);
            until(|| state(&d.w).customization.drawer.is_none() && d.w.drawer.is_closed(), "Escape closes the drawer");
        }
    }
    for swapped in [true, false] {
        if state(&d.w).phone.swapped != swapped { menu(&mut d); click(&mut d, "phone-swap"); }
        for setting in ["size", "strength"] {
            let slider = d.named(&format!("component-slider-{setting}")).downcast::<gtk::Scale>().unwrap();
            let before = slider.value();
            let range = slider.range_rect();
            let (start,end) = slider.slider_range();
            let midpoint = (start+end) as f32*0.5;
            let horizontal = slider.orientation() == gtk::Orientation::Horizontal;
            let (origin,length,cross) = if horizontal {
                (range.x() as f32,range.width() as f32,range.y() as f32+range.height() as f32*0.5)
            } else { (range.y() as f32,range.height() as f32,range.x() as f32+range.width() as f32*0.5) };
            let target = origin+length*if midpoint > origin+length*0.5 {0.2} else {0.8};
            let point = |position| {
                let [x,y] = if horizontal {[position,cross]} else {[cross,position]};
                let point = slider.compute_point(&d.w.window,&gtk::graphene::Point::new(x,y)).unwrap();
                [point.x(),point.y()]
            };
            let from = point(midpoint);
            let to = point(target);
            d.input.perform(serde_json::json!([contact("mouse", "down", from), contact("mouse", "move", to)]));
            let popup = d.named("brush-slider-preview").downcast::<gtk::Popover>().unwrap();
            assert!(popup.is_mapped());
            let content = popup.child().unwrap();
            let top = screen_point(&content, &d.w.window, [0., 0.]);
            let bottom = screen_point(&content, &d.w.window, [1., 1.]);
            let surface = screen_point(d.w.surface.upcast_ref(), &d.w.window, [0., 0.]);
            assert!(top[0] >= surface[0] && top[1] >= surface[1]
                && bottom[0] <= surface[0] + d.w.surface.width() as f32
                && bottom[1] <= surface[1] + d.w.surface.height() as f32,
                "{setting}/{swapped}: preview stays inside the window: {top:?}..{bottom:?}");
            let label = find_named(&content, "slider-preview-label").unwrap();
            let label_top = screen_point(&label, &d.w.window, [0., 0.]);
            let label_bottom = screen_point(&label, &d.w.window, [1., 1.]);
            assert!(label_top[0] >= top[0] && label_top[1] >= top[1]
                && label_bottom[0] <= bottom[0] && label_bottom[1] <= bottom[1],
                "slider readout stays inside the preview");
            let range = slider.range_rect();
            let (start, end) = slider.slider_range();
            let horizontal = slider.orientation() == gtk::Orientation::Horizontal;
            let point = if horizontal { [(start+end) as f32*0.5, range.y() as f32+range.height() as f32*0.5] }
                else { [range.x() as f32+range.width() as f32*0.5,(start+end) as f32*0.5] };
            let handle = slider.compute_point(&d.w.window, &gtk::graphene::Point::new(point[0],point[1])).unwrap();
            let axis = usize::from(!horizontal);
            let center = (top[axis]+bottom[axis])*0.5;
            let half = (bottom[axis]-top[axis])*0.5;
            let extent = if horizontal { d.w.window.width() } else { d.w.window.height() } as f32;
            let expected = [handle.x(),handle.y()][axis].clamp(8.+half,extent-8.-half);
            assert!((center-expected).abs() <= 2.,
                "{setting}/{swapped}: preview follows actual transformed slider handle: {center} vs {expected}");
            capture_current(&mut d,&format!("slider-{setting}-{swapped}"));
            d.input.perform(serde_json::json!([contact("mouse", "up", to)]));
            assert_ne!(slider.value(), before, "the real slider changes its brush value");
        }
    }
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint, "drawers and sliders do not paint");
    d.finish();
}

fn mouse_paint(d: &mut Driver, phase: &str) {
    d.w.dispatch(UiAction::Invoke { command: CommandId::DrawingBrush });
    d.w.dispatch(UiAction::SetColor { rgba: [0., 0., 0., 1.] });
    d.w.dispatch(ToolbarControl::BrushSizeSlider.slider().unwrap().action(40.));
    until(|| d.w.gpu.borrow().as_ref().is_some_and(|g| {
        let engine = g.session.engine();
        engine.backend().paint_ready(engine.document(), engine.brush(), false)
    }), "phone mouse brush is ready");
    let area = presentation(d).work_area;
    let from = [area.x + area.width * 0.4, area.y + area.height * 0.5];
    let to = [area.x + area.width * 0.6, from[1]];
    for point in [from, to] {
        assert_eq!(d.w.surface.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT),
            Some(d.w.area.clone().upcast()), "{phase}: canvas receives native input");
    }
    let pixels = |d: &Driver| {
        let image = ui_session(&d.w).engine().backend().capture().unwrap();
        let scale = [image.width as f32 / d.w.surface.width() as f32, image.height as f32 / d.w.surface.height() as f32];
        let mut samples = Vec::new();
        for step in 0..9 {
            let x = ((from[0] + (to[0] - from[0]) * (step as f32 + 1.) / 10.) * scale[0]) as usize;
            let y = (from[1] * scale[1]) as usize;
            samples.push(image.bytes[y * image.stride as usize + x * 4..][..4].to_vec());
        }
        samples
    };
    until(|| ui_session(&d.w).engine().backend().frames_idle(), "phone mouse starting pixels are published");
    let before = pixels(d);
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    let committed = ui_session(&d.w).engine().metrics().committed_strokes;
    let point = |p: [f32; 2]| screen_point(d.w.surface.upcast_ref(), &d.w.window,
        [p[0] / d.w.surface.width() as f32, p[1] / d.w.surface.height() as f32]);
    let mut events = vec![contact("mouse", "down", point(from))];
    events.extend((1..=20).map(|step| contact("mouse", "move",
        point([from[0] + (to[0] - from[0]) * step as f32 / 20., from[1]]))));
    events.push(contact("mouse", "up", point(to)));
    d.input.perform(serde_json::Value::Array(events));
    until(|| ui_session(&d.w).engine().metrics().committed_strokes > committed
        && !ui_session(&d.w).engine().has_pending_document_edits()
        && ui_session(&d.w).engine().backend().frames_idle(), "phone mouse stroke commits");
    until(|| pixels(d) != before, "phone mouse stroke changes canvas pixels");
    let painted = pixels(d);
    assert!(painted.iter().zip(&before).filter(|(a, b)| a != b).count() >= 7,
        "mouse paints along the canvas path: {before:?} -> {painted:?}");
    capture(d, &format!("mouse-stroke-{phase}"));
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| ui_session(&d.w).engine().checkpoint() == checkpoint
        && ui_session(&d.w).engine().backend().frames_idle() && pixels(d) == before,
        "one Undo restores the mouse stroke pixels and checkpoint");
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_window_move --native-storage; 1600x1000 viewport"]
fn native_phone_window_move() {
    let mut d = super::phone_motion::setup("art.capycanvas.PhoneWindowMove");
    let monitor = d.w.window.surface().unwrap().display().monitor_at_surface(&d.w.window.surface().unwrap()).unwrap().geometry();
    assert!(monitor.width() >= 1400 && monitor.height() >= 950);
    let observed = Rc::new(Cell::new(None));
    let motion = gtk::EventControllerMotion::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    let w = d.w.clone();
    motion.connect_motion(glib::clone!(#[weak] w, #[strong] observed, move |_, x, y| {
        let point = w.window.compute_point(&w.surface, &gtk::graphene::Point::new(x as f32, y as f32)).unwrap();
        observed.set(Some([point.x(), point.y()]));
    }));
    d.w.window.add_controller(motion);
    d.w.window.unmaximize();
    d.w.window.set_default_size(412, 700);
    until(|| !d.w.window.is_maximized() && [d.w.surface.width(), d.w.surface.height()] == [412, 700],
        "unmaximized phone window is ready");
    let mut origin = None;
    'scan: for y in (100..monitor.height()).step_by(100) {
        for x in (100..monitor.width()).step_by(100) {
            observed.set(None);
            d.input.perform(serde_json::json!([{"point":[x - 1,y]},{"point":[x,y]}]));
            if let Some(local) = observed.get() {
                origin = Some([x as f32 - local[0], y as f32 - local[1]]);
                break 'scan;
            }
        }
    }
    let mut origin = origin.expect("calibrate private window position from native pointer motion");
    for size in [[412, 700], [700, 412]] {
        d.w.window.set_default_size(size[0], size[1]);
        until(|| [d.w.surface.width(), d.w.surface.height()] == size, "phone movement orientation is ready");
        let probe = [origin[0] + 200., origin[1] + 200.];
        observed.set(None);
        d.input.perform(serde_json::json!([{"point":[probe[0]-1.,probe[1]]},{"point":probe}]));
        let local = observed.get().expect("pointer inside resized phone window");
        origin = [probe[0] - local[0], probe[1] - local[1]];
        for device in ["mouse", "touch"] {
            for bar in 0..2 {
                let layout = presentation(&d);
                let bounds = layout.bars[bar];
                let point = (1..100).map(|step| {
                    let t = step as f32 / 100.;
                    if size[0] < size[1] { [bounds.x + bounds.width * t, bounds.y + bounds.height * 0.5] }
                    else { [bounds.x + bounds.width * 0.5, bounds.y + bounds.height * t] }
                }).find(|point| {
                    !layout.tool_hits.iter().chain([&layout.capy_hit, &layout.size, &layout.strength])
                        .chain(layout.controls.iter()).any(|b| point[0] >= b.x && point[0] <= b.x + b.width
                            && point[1] >= b.y && point[1] <= b.y + b.height)
                }).expect("phone bar has empty draggable space");
                let from = [origin[0] + point[0], origin[1] + point[1]];
                let delta = if bar == 0 { [60., 30.] } else { [-60., -30.] };
                let to = [from[0] + delta[0], from[1] + delta[1]];
                let before = layer_ui::durable_layout(&state(&d.w).workspace.layout);
                let checkpoint = ui_session(&d.w).engine().checkpoint();
                d.input.perform(serde_json::json!([contact(device,"down",from),
                    contact(device,"move",[from[0]+delta[0]*0.3,from[1]+delta[1]*0.3]),
                    contact(device,"move",to),contact(device,"up",to)]));
                let probe = [origin[0] + size[0] as f32 * 0.5, origin[1] + size[1] as f32 * 0.5];
                observed.set(None);
                d.input.perform(serde_json::json!([{"point":[probe[0]-1.,probe[1]]},{"point":probe}]));
                let local = observed.get().expect("pointer inside moved phone window");
                let moved = [probe[0] - local[0], probe[1] - local[1]];
                assert!((moved[0] - origin[0]) * delta[0].signum() > 30.
                    && (moved[1] - origin[1]) * delta[1].signum() > 10.,
                    "{size:?}/{device}/bar {bar}: actual origin {origin:?} -> {moved:?}, expected delta {delta:?}");
                assert_eq!(layer_ui::durable_layout(&state(&d.w).workspace.layout), before);
                assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
                assert!(d.w.chrome_contacts.borrow().is_empty());
                origin = moved;
            }
        }
    }
    d.finish();
}

fn menu_pages(d: &mut Driver) {
    let swapped = state(&d.w).phone.swapped;
    let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    let main = stack.visible_child_name().unwrap();
    let initial = d.w.phone.placement.borrow().as_ref().unwrap().bounds;
    assert_eq!(main.as_str(), "main");
    menu_bounds(d, initial);
    menu_semantics(d, &stack);
    let prefs = d.named("phone-preferences");
    let search = d.named("phone-search");
    inside(d, &prefs);
    inside(d, &search);
    assert!(prefs.tooltip_text().is_some_and(|label| !label.is_empty()));
    assert!(gtk::test_accessible_has_property(&prefs, gtk::AccessibleProperty::Label));
    let a = prefs.compute_bounds(&d.w.surface).unwrap();
    let b = search.compute_bounds(&d.w.surface).unwrap();
    assert!((a.y() + a.height() * 0.5 - b.y() - b.height() * 0.5).abs() <= 1.,
        "Preferences gear is beside Search");
    let phone = presentation(d);
    assert!((a.width()-phone.tile).abs() <= 1. && (a.height()-phone.tile).abs() <= 1.,
        "Preferences uses the same square tile as the bar");
    assert!(prefs.has_css_class("header-tool") && prefs.parent().unwrap().has_css_class("header-bar"));
    let icon = prefs.first_child().unwrap().downcast::<gtk::Image>().unwrap();
    assert!((icon.pixel_size() as f32-phone.icon).abs() <= 1., "Preferences uses the bar icon size");
    assert_eq!(search.type_().name(), "GtkModelButton");
    assert!((7. ..=9.).contains(&(a.x()-b.x()-b.width())), "Preferences is separate at Search's trailing end");
    let submenus = ui_session(&d.w).phone_menu_columns()[1].sections.iter().flatten()
        .filter(|item| !item.sections.is_empty()).take(2).map(|item| item.label.clone()).collect::<Vec<_>>();
    assert_eq!(submenus.len(), 2);
    for (index, label) in submenus.into_iter().enumerate() {
        let row = widgets(&d.named("phone-menu-column-1")).find(|widget|
            widget.is_mapped() && widget.type_().name() == "GtkModelButton"
                && widget.property::<String>("text") == label).unwrap();
        d.click(&row);
        until(|| stack.visible_child_name().as_deref() == Some(format!("page-0-{index}").as_str())
            && !stack.is_transition_running(), "phone submenu slides in place");
        submenu(d, &stack, &format!("submenu-{index}-{swapped}"), initial);
        assert!(!d.w.popovers.borrow().iter().filter_map(|popup| popup.upgrade()).any(|popup| popup.is_mapped()),
            "phone submenus do not open a floating popover");
        click(d, "phone-menu-back");
        until(|| stack.visible_child_name().as_deref() == Some("main") && !stack.is_transition_running(),
            "phone submenu back row returns to the root");
        capture(d, &format!("submenu-{index}-back-{swapped}"));
        menu_bounds(d, initial);
        menu_semantics(d, &stack);
    }
    click(d, "workspace-switcher-options");
    until(|| stack.visible_child_name().as_deref() == Some("page-0-7-1-0") && !stack.is_transition_running(),
        "workspace options navigates the menu to Window Workspaces");
    submenu(d, &stack, &format!("workspaces-{swapped}"), initial);
    assert!(!d.w.popovers.borrow().iter().filter_map(|popup| popup.upgrade()).any(|popup| popup.is_mapped()),
        "workspace options stays inside Capy's menu");
    for page in ["page-0-7", "main"] {
        click(d, "phone-menu-back");
        until(|| stack.visible_child_name().as_deref() == Some(page) && !stack.is_transition_running(),
            "Workspaces back row follows the menu hierarchy");
        capture(d, &format!("workspaces-back-{page}-{swapped}"));
        menu_bounds(d, initial);
        menu_semantics(d, &stack);
    }
    let point = d.point(&prefs);
    d.input.perform(serde_json::json!([{"point":point}]));
    assert!(prefs.state_flags().contains(gtk::StateFlags::PRELIGHT));
    capture_current(d,&format!("preferences-hover-{swapped}"));
    d.input.perform(serde_json::json!([contact("mouse","down",point)]));
    assert!(prefs.state_flags().contains(gtk::StateFlags::ACTIVE));
    assert!(!d.w.preferences.dialog.is_mapped(), "press capture does not activate Preferences");
    capture_current(d,&format!("preferences-pressed-{swapped}"));
    assert!(!d.w.preferences.dialog.is_mapped(), "Preferences opens on release");
    d.input.perform(serde_json::json!([contact("mouse","up",point)]));
    until(|| d.w.preferences.dialog.is_mapped(), "Preferences gear opens Settings");
    d.w.preferences.dialog.close();
    until(|| !d.w.preferences.dialog.is_mapped(), "Settings closes");
    if !state(&d.w).phone.menu { menu(d); }
    let retired = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    d.input.key(0xff1b);
    until(|| !state(&d.w).phone.menu && d.w.phone.placement.borrow().is_none(), "Escape closes the menu before replacement");
    menu(d);
    let current = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    assert_ne!(retired, current);
    retired.set_visible_child_name("page-0-0");
    d.w.surface.queue_allocate();
    capture(d, &format!("retired-menu-stack-{swapped}"));
    assert_eq!(current.visible_child_name().as_deref(), Some("main"));
    menu_semantics(d, &current);
    assert!(d.named("phone-menu-column-0").is_mapped());
    let placement = d.w.phone.placement.borrow().as_ref().unwrap().clone();
    let menu_root = d.named("phone-menu");
    for (index, expected) in placement.columns.iter().enumerate() {
        let scroll = d.named(&format!("phone-menu-scroll-{index}"));
        assert!(scroll.is_mapped());
        let actual = scroll.compute_bounds(&menu_root).unwrap();
        assert!([actual.x()-expected.x,actual.y()-expected.y,actual.width()-expected.width,actual.height()-expected.height]
            .into_iter().all(|difference| difference.abs() <= 1.), "retired stack leaves current root column geometry intact");
    }
    d.w.dispatch(UiAction::Invoke { command: CommandId::ActualPixels });
    d.w.dispatch(UiAction::Invoke { command: CommandId::ZoomIn });
    new_photo::ready(&d.w);
    let zoom = state(&d.w).camera.zoom;
    assert!(zoom > 1. && !state(&d.w).phone.menu);
    let selected = [CommandId::FitCanvas,CommandId::ActualPixels].map(|command| ui_session(&d.w).command(command).selected);
    menu(d);
    let camera_stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    let label = ApplicationMenu::File.localized_label(&d.w.localization());
    let row = widgets(&camera_stack.visible_child().unwrap()).find(|widget| widget.is_mapped()
        && widget.type_().name()=="GtkModelButton" && widget.property::<String>("text")==label.as_ref()).unwrap();
    d.click(&row);
    until(|| camera_stack.visible_child_name().as_deref()==Some("page-0-0") && !camera_stack.is_transition_running(),
        "File opens at free zoom");
    let bounds = d.w.phone.placement.borrow().as_ref().unwrap().bounds;
    submenu(d,&camera_stack,&format!("camera-close-file-{swapped}"),bounds);
    d.w.dispatch(UiAction::Invoke { command: CommandId::ZoomIn });
    until(|| !state(&d.w).phone.menu && d.w.phone.placement.borrow().is_none(), "camera-only command closes the menu");
    assert!(state(&d.w).camera.zoom > zoom);
    assert_eq!([CommandId::FitCanvas,CommandId::ActualPixels].map(|command| ui_session(&d.w).command(command).selected), selected);
    menu(d);
    let reopened = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    capture(d,&format!("camera-reopened-root-{swapped}"));
    assert_eq!(reopened.visible_child_name().as_deref(),Some("main"));
    menu_semantics(d,&reopened);
    assert!(d.named("phone-menu-column-0").is_mapped());
    menu_bounds(d,d.w.phone.placement.borrow().as_ref().unwrap().bounds);
    d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    new_photo::ready(&d.w);
    menu(d);
}

fn submenu(d: &mut Driver, stack: &gtk::Stack, name: &str, initial: Bounds) {
    capture(d, name);
    menu_bounds(d, initial);
    menu_semantics(d, stack);
    let scroll = d.named("phone-menu-scroll-1").downcast::<gtk::ScrolledWindow>().unwrap();
    inside(d, scroll.upcast_ref());
    let viewport = scroll.compute_bounds(&d.w.surface).unwrap();
    let menu = initial;
    let within = |bounds: [f32;4]| bounds[0] >= menu.x-1. && bounds[1] >= menu.y-1.
        && bounds[2] <= menu.x+menu.width+1. && bounds[3] <= menu.y+menu.height+1.;
    let port = [viewport.x(),viewport.y(),viewport.x()+viewport.width(),viewport.y()+viewport.height()];
    assert!(within(port), "submenu viewport stays inside the published menu: {viewport:?}, {menu:?}");
    let page = stack.visible_child().unwrap();
    let bounds = page.compute_bounds(&d.w.surface).unwrap();
    assert!(bounds.x() >= port[0]-1. && bounds.x()+bounds.width() <= port[2]+1.,
        "submenu content has no horizontal overflow: {bounds:?}, {viewport:?}");
    let horizontal = scroll.hadjustment();
    assert!(horizontal.upper() <= horizontal.page_size()+0.5, "submenu never scrolls horizontally");
    let clipped = [bounds.x().max(port[0]),bounds.y().max(port[1]),
        (bounds.x()+bounds.width()).min(port[2]),(bounds.y()+bounds.height()).min(port[3])];
    assert!(clipped[2]>clipped[0] && clipped[3]>clipped[1] && within(clipped),
        "visible submenu content stays inside the published menu");
    let back = widgets(&page).find(|widget| widget.is_mapped() && widget.widget_name()=="phone-menu-back").unwrap();
    inside(d,&back);
    let back = back.compute_bounds(&scroll).unwrap();
    assert!(back.x() >= -1. && back.y() >= -1. && back.x()+back.width() <= scroll.width() as f32+1.
        && back.y()+back.height() <= scroll.height() as f32+1., "the whole Back row is visible");
    scroll_overflow(d);
}

fn menu_semantics(d: &Driver, stack: &gtk::Stack) {
    let model = ui_session(&d.w).phone_menu_columns()[1].clone();
    let pages = PhoneMenuNavigation::pages(&model);
    let name = stack.visible_child_name().unwrap();
    let expected = pages.iter().find(|page| page.name()==name.as_str()).unwrap();
    let child = stack.visible_child().unwrap();
    assert_eq!(stack.page(&child).title().as_deref(), Some(expected.menu.title.as_str()));
    let mut rows = widgets(&child).filter(|widget| widget.is_mapped() && widget.type_().name()=="GtkModelButton");
    if let Some(parent) = expected.parent_name() {
        let back = rows.next().unwrap();
        assert_eq!(back.widget_name(), "phone-menu-back");
        assert_eq!(back.property::<String>("text"), expected.menu.title);
        assert_eq!(back.property::<Option<String>>("menu-name").as_deref(), Some(parent.as_str()));
    }
    let rows = rows.collect::<Vec<_>>();
    let items = expected.menu.sections.iter().enumerate().flat_map(|(s, section)| section.iter().enumerate()
        .filter(|(_,item)| !item.is_heading()).map(move |(i,item)| (s,i,item))).collect::<Vec<_>>();
    assert_eq!(rows.len(), items.len(), "{name}: the whole native page matches the shared menu");
    for (row,(section,index,item)) in rows.iter().zip(items) {
        assert_eq!(row.property::<String>("text"), item.label, "{name}: actual menu contents");
        if item.action.is_none() && item.enabled && !item.sections.is_empty() {
            let mut path = expected.path.clone();
            path.push([section,index]);
            assert_eq!(row.property::<Option<String>>("menu-name"), Some(PhoneMenuNavigation::page_name(&path)),
                "{name}: native forward destination");
        }
    }
    menu_separator(d,name.as_str());
}

fn menu_bounds(d: &Driver, initial: Bounds) {
    assert_eq!(d.w.phone.placement.borrow().as_ref().unwrap().bounds, initial,
        "navigation preserves the initial published menu bounds");
    for widget in std::iter::once(d.named("phone-menu")).chain((0..2)
        .map(|index| d.named(&format!("phone-menu-scroll-{index}"))).filter(|widget| widget.is_mapped())) {
        let bounds = widget.compute_bounds(&d.w.surface).unwrap();
        assert!(bounds.x() >= initial.x-1. && bounds.y() >= initial.y-1.
            && bounds.x()+bounds.width() <= initial.x+initial.width+1.
            && bounds.y()+bounds.height() <= initial.y+initial.height+1.,
            "{} stays inside the initial menu bounds: {bounds:?}, {initial:?}", widget.widget_name());
    }
}

fn menu_separator(d: &Driver, page: &str) {
    let model = ui_session(&d.w).phone_menu_columns()[1].clone();
    let mut navigation = PhoneMenuNavigation::default();
    assert!(navigation.select(&model,page));
    let placement = d.w.phone.placement.borrow().as_ref().unwrap().clone();
    let expected = navigation.geometry(&placement).separator;
    let separator = d.named("phone-menu-separator").downcast::<gtk::Separator>().unwrap();
    let shown = expected.width > 0. && expected.height > 0.;
    assert_eq!(separator.is_mapped(),shown,"{page}: section separator visibility matches the projected panes");
    assert_eq!(separator.is_child_visible(),shown);
    if shown {
        inside(d,separator.upcast_ref());
        let native = widgets(&d.named("phone-menu-column-0")).find(|widget| widget.is_mapped() && widget.is::<gtk::Separator>()).unwrap();
        assert_eq!(separator.color(),native.color(),"the pane boundary matches the native menu separator foreground");
        assert_eq!(separator.opacity(),native.opacity(),"the pane boundary matches the native menu separator opacity");
        let actual = separator.compute_bounds(&d.named("phone-menu")).unwrap();
        assert!([actual.x()-expected.x,actual.y()-expected.y,actual.width()-expected.width,actual.height()-expected.height]
            .into_iter().all(|difference| difference.abs() <= 1.), "native section separator uses shared geometry: {actual:?}, {expected:?}");
        assert_eq!(separator.orientation(),if expected.width > expected.height {gtk::Orientation::Horizontal} else {gtk::Orientation::Vertical});
    }
}
