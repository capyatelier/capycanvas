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
    click_widget(d, &widget);
}

fn click_widget(d: &mut Driver, widget: &gtk::Widget) {
    if let Some(scroll) = std::iter::successors(widget.parent(), |parent| parent.parent())
        .find_map(|parent| parent.downcast::<gtk::ScrolledWindow>().ok()) {
        if widget.is_focusable() {
            assert!(widget.grab_focus());
            until(|| widget.compute_bounds(&scroll).is_some_and(|bounds|
                bounds.y() >= -1. && bounds.y() + bounds.height() <= scroll.height() as f32 + 1.),
                "scroll the actual menu action into view before clicking");
        }
    }
    d.click(widget);
}

fn capture_current(d: &mut Driver, name: &str) {
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        let theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "light".into());
        let capture_name = format!("phone-{}x{}-{theme}-{name}", d.w.surface.width(), d.w.surface.height());
        d.input.perform(serde_json::json!([{ "capture_window": {
            "name": &capture_name,
            "maximized": d.w.window.is_maximized(),
        } }]));
        super::phone_motion::export_allocations(&d.w,name,&capture_name);
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
    let view = state(&d.w);
    let point = view.phone_presentation([d.w.surface.width() as f32, d.w.surface.height() as f32]).filter(|_| !view.phone.hidden).map_or_else(|| d.point(&d.named("zen-capy")), |phone| {
        if phone.edge != Edge::Top { [phone.capy.x + phone.tile * 0.5, phone.capy.y + phone.tile + 4.] }
        else { [if view.phone.swapped { phone.capy.x - 4. } else { phone.capy.x + phone.tile + 4. }, phone.capy.y + phone.tile * 0.5] }
    });
    d.input.perform(serde_json::json!([{ "point": point }]));
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
    for name in ["component-slider-size", "component-slider-strength"] {
        let slider = d.named(name);
        let parent = slider.parent().unwrap();
        assert_eq!(parent.first_child().as_ref(), Some(&slider), "phone sliders create no grip widget");
        assert!(slider.next_sibling().is_none());
        let track = slider.compute_bounds(&parent).unwrap();
        assert!(track.x().abs() <= 1. && track.y().abs() <= 1.
            && (track.width() - parent.width() as f32).abs() <= 1.
            && (track.height() - parent.height() as f32).abs() <= 1., "the phone slider uses its whole bounds");
    }
    let first = state(&d.w).workspace.layout.phone.unwrap().into_iter().find(|tile| tile.control != ToolbarControl::Divider).unwrap();
    let button = d.named(&format!("phone-tool-{}", first.id));
    let bounds = button.compute_bounds(&d.w.surface).unwrap();
    let picked = d.w.surface.pick((bounds.x()+bounds.width()*0.5) as f64,
        (bounds.y()+bounds.height()*0.5) as f64, gtk::PickFlags::DEFAULT).unwrap();
    assert!(picked == button || picked.is_ancestor(&button), "phone controls receive native input");
    for (name, expected) in [("phone-tools-chip", phone.chips[0]), ("phone-sliders-chip", phone.chips[1]),
        ("phone-bar-tools", phone.bars[0]), ("phone-bar-capy", phone.bars[1]), ("phone-title-band", phone.title)] {
        let widget = d.named(name);
        assert_eq!(widget.is_mapped(), expected.width > 0. && expected.height > 0., "{name}: nonempty shared geometry controls visibility");
        if expected.width <= 0. || expected.height <= 0. { continue; }
        inside(d, &widget);
        if name == "phone-title-band" {
            assert!(widget.measure(gtk::Orientation::Horizontal,-1).1 as f32 <= expected.width
                && widget.measure(gtk::Orientation::Vertical,-1).1 as f32 <= expected.height,
                "the undecorated title handle does not expand shared geometry");
        }
        let bounds = widget.compute_bounds(&d.w.surface).unwrap();
        assert!([bounds.x()-expected.x,bounds.y()-expected.y,bounds.width()-expected.width,bounds.height()-expected.height]
            .into_iter().all(|difference| difference.abs() <= 1.), "{name} uses shared continuous geometry");
    }
    for (index, chip) in [(0, phone.chips[0]), (1, phone.chips[1])] {
        let parts = if index == 0 { phone.tools.to_vec() } else { vec![phone.capy, phone.size, phone.strength, phone.settings] };
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
    let scale = (d.w.surface.width().min(d.w.surface.height()) as f32 / 320.).min(1.);
    assert!((gap - 2. * scale).abs() <= 0.01, "sliders share one chip with the scaled internal gap");
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

fn menu_row_height(d: &Driver) -> f32 {
    let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    widgets(&stack.child_by_name("main").unwrap()).find(|row|
        row.css_name() == "modelbutton" && row.widget_name() != "phone-drawing").unwrap()
        .measure(gtk::Orientation::Vertical, -1).1 as f32
}

fn menu(d: &mut Driver) {
    if !state(&d.w).phone.menu { click(d, "zen-capy"); }
    until(|| state(&d.w).phone.menu && d.w.drawer.placement().is_some()
        && find_named(d.w.window.upcast_ref(), "phone-menu-scroll").is_some_and(|w| w.is_mapped() && w.width() > 0),
        "Capy's list and fixed strip are allocated");
    let swapped = state(&d.w).phone.swapped;
    capture(d, &format!("menu-open-{swapped}"));
    let root = d.named("phone-menu");
    inside(d, &root);
    let placement = d.w.drawer.placement().as_ref().unwrap().clone();
    assert_eq!(placement.columns.len(), 1);
    let geometry = presentation(d).menu_geometry(&placement, menu_row_height(d));
    for (name, expected) in [("phone-menu-scroll", geometry.list), ("phone-menu-separator", geometry.separator)] {
        let widget = d.named(name);
        inside(d, &widget);
        let actual = widget.compute_bounds(&root).unwrap();
        assert!([actual.x() - expected.x, actual.y() - expected.y, actual.width() - expected.width,
            actual.height() - expected.height].into_iter().all(|delta| delta.abs() <= 1.),
            "{name} uses shared list/strip geometry: {actual:?}, {expected:?}");
    }
    let model = ui_session(&d.w).phone_menu().unwrap();
    assert_eq!(model.strip.len(), 5);
    for ((name, expected), item) in ["phone-preferences", "phone-search", "phone-finger", "phone-zen", "phone-swap"]
        .into_iter().zip(geometry.buttons).zip(model.strip) {
        let button = d.named(name);
        inside(d, &button);
        let actual = button.compute_bounds(&root).unwrap();
        assert!([actual.x() - expected.x, actual.y() - expected.y, actual.width() - expected.width,
            actual.height() - expected.height].into_iter().all(|delta| delta.abs() <= 1.),
            "{name} keeps its fixed icon-strip slot");
        assert!((actual.width() - menu_row_height(d)).abs() <= 1. && (actual.height() - menu_row_height(d)).abs() <= 1.);
        assert!(button.has_css_class("header-tool"));
        assert_eq!(button.first_child().and_downcast::<gtk::Image>().unwrap().pixel_size(), geometry.icon.round() as i32);
        assert!(button.tooltip_text().is_some_and(|text| !text.is_empty()));
        assert!(gtk::test_accessible_has_property(&button, gtk::AccessibleProperty::Label));
        assert_eq!(button.is_sensitive(), item.enabled);
    }
    assert_eq!(d.named("phone-swap").property::<bool>("active"), state(&d.w).phone.swapped);
    assert_eq!(d.named("phone-finger").property::<bool>("active"), ui_session(&d.w).finger_drawing());
    let scroll = d.named("phone-menu-scroll").downcast::<gtk::ScrolledWindow>().unwrap();
    scroll_overflow(d);
    scroll.vadjustment().set_value(0.);
    let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    menu_semantics(d, &stack);
}

fn menu_page_name(d: &Driver, title: &str) -> String {
    ui_session(&d.w).phone_menu().unwrap().view(&drawing_selector(d), Platform::Gtk, &d.w.localization()).pages.into_iter().find(|page| page.title == title).unwrap().id.name()
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
    d.w.window.fullscreen();
    until(|| d.w.window.is_fullscreen() && !state(&d.w).phone.movable
        && presentation(&d).title.width == 0. && !d.named("phone-title-band").is_mapped(), "fullscreen removes the movable title band");
    d.w.window.unfullscreen();
    until(|| !d.w.window.is_fullscreen() && state(&d.w).phone.movable
        && d.named("phone-title-band").is_mapped() == (presentation(&d).title.width > 0.),
        "leaving fullscreen restores movable phone geometry");
    frame(&d);
    mouse_paint(&mut d, "normal");
    let before = presentation(&d).capy;
    menu(&mut d);
    capture(&mut d, "menu");
    menu_pages(&mut d);
    click(&mut d, "phone-swap");
    until(|| state(&d.w).phone.swapped, "Swap Sides mirrors the bars");
    assert_ne!(presentation(&d).capy, before);
    assert!(state(&d.w).phone.menu);
    d.input.key(0xff1b);
    until(|| !state(&d.w).phone.menu, "close the kept-open menu before capturing the swapped bars");
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
        assert_eq!(child.is_mapped(), child.widget_name() == "phone-sliders-chip"
            || (child.widget_name() == "phone-title-band" && presentation(&d).title.width > 0.),
            "only retained phone input stays mapped: {}", child.widget_name());
    }
    assert!(d.w.drawer.placement().is_none());
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
        let view = state(&d.w).phone_tools().into_iter().find(|view| format!("phone-tool-{}",view.id.unwrap()) == tile).unwrap();
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
            let restored = adjustment.clone();
            let frames = sample_after_paint(&scroll, move || restored.value());
            adjustment.set_value(before);
            scroll.queue_draw();
            until(|| frames.samples.borrow().iter().any(|sample| (sample.state - before).abs() < 0.01),
                "the restored scroller allocation reaches native after-paint before the next input");
            drop(frames.finish());
        }
    }
}

fn color_drawer(d: &Driver) {
    let wheel = d.named("color-wheel").downcast::<crate::tool_panels::ColorWheel>().unwrap();
    let content = d.named("drawer-panel-Color");
    let (diameter, origin) = wheel.drawing_bounds();
    assert!(diameter >= 127.5 && origin[1] >= -0.5 && origin[1] + diameter <= wheel.height() as f32 + 0.5);
    assert!(wheel.width() <= content.width());
    let scroll = wheel.ancestor(gtk::ScrolledWindow::static_type()).unwrap();
    assert_eq!(scroll.widget_name(), "color-drawer-scroll", "the wheel has only the whole drawer scroller");
    let palettes = d.named("drawer-panel-Palettes");
    let adjustment = scroll.clone().downcast::<gtk::ScrolledWindow>().unwrap().vadjustment();
    if adjustment.upper() <= adjustment.page_size() + 0.5 { inside(d, &palettes); }
    else { assert!(diameter <= 128.5, "whole-drawer scrolling starts only at the wheel minimum"); }
    let top = wheel.compute_point(&scroll, &gtk::graphene::Point::new(origin[0], origin[1])).unwrap();
    assert!(top.x() >= -0.5 && top.y() >= -0.5
        && top.x() + diameter <= scroll.width() as f32 + 0.5
        && top.y() + diameter <= scroll.height() as f32 + 0.5, "the whole wheel fits its drawer viewport");
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

fn drawer_selection_colors(d: &mut Driver) {
    let captures = std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").expect("drawer colour captures"));
    let original = (state(&d.w).theme, state(&d.w).settings.transparency);
    let mut samples = Vec::new();
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for level in [layer_ui::Transparency::Off, layer_ui::Transparency::Medium] {
            use_transparency(&d.w, level);
            d.w.dispatch(UiAction::Invoke { command: CommandId::DrawingBrush });
            let opener = tool(d, ToolbarControl::Command { command: CommandId::DrawingBrush });
            click(d, &opener);
            inventory_drawer_ready(d);
            let row = widgets(d.w.window.upcast_ref()).find(|widget| widget.is_mapped()
                && widget.has_css_class("brush-choice") && widget.has_css_class("selected-tool")).unwrap();
            super::histogram::scroll_to(&row);
            let tile = d.named(&opener);
            assert!(tile.has_css_class("selected-tool") && tile.has_css_class("drawer-open"));
            let points = [tile, row].map(|widget| {
                let bounds = widget.compute_bounds(&d.w.window).unwrap();
                [bounds.x() + 3., bounds.y() + bounds.height() * 0.5]
            });
            let name = format!("selection-{theme:?}-{level:?}").to_lowercase();
            capture(d, &name);
            let fixture_theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "light".into());
            let path = captures.join(format!("phone-{}x{}-{fixture_theme}-{name}.png", d.w.surface.width(), d.w.surface.height()));
            let size = [d.w.window.width() as f32, d.w.window.height() as f32];
            let pixels = glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
                let mut reader = png::Decoder::new(std::fs::File::open(path).unwrap()).read_info().unwrap();
                let mut bytes = vec![0; reader.output_buffer_size()];
                let frame = reader.next_frame(&mut bytes).unwrap();
                assert_eq!(frame.color_type, png::ColorType::Rgba);
                points.map(|point| {
                    let [x, y] = [0, 1].map(|axis| (point[axis] / size[axis]
                        * [frame.width, frame.height][axis] as f32).floor() as usize);
                    let offset = (y * frame.width as usize + x) * 4;
                    <[u8; 4]>::try_from(&bytes[offset..offset + 4]).unwrap()
                })
            })).unwrap();
            samples.push((name, points, pixels, if level == layer_ui::Transparency::Off { 3 } else { 6 }));
            d.input.key(0xff1b);
            until(|| state(&d.w).customization.drawer.is_none() && d.w.drawer.is_closed(), "Escape closes the sampled drawer");
        }
    }
    d.w.dispatch(UiAction::SetTheme { theme: Some(original.0) });
    use_transparency(&d.w, original.1);
    assert!(samples.iter().all(|(_, _, pixels, tolerance)| pixels[0][..3].iter().zip(&pixels[1][..3])
        .all(|(tile, row)| tile.abs_diff(*row) <= *tolerance)), "selected tile and drawer row differ: {samples:#?}");
}

pub(super) fn assert_group_chip(button: &gtk::Button, context: &str) {
    let content = button.child().unwrap();
    let icon = content.first_child().unwrap().compute_bounds(button).unwrap();
    let label = content.last_child().unwrap().downcast::<gtk::Label>().unwrap();
    let text = label.compute_bounds(button).unwrap().x() + label.layout_offsets().0 as f32;
    let gap = text - icon.x() - icon.width();
    assert!((-1. ..=8.).contains(&gap), "{context}: rendered label stays beside its icon: {gap}");
    let cell = button.parent().unwrap().downcast::<gtk::FlowBoxChild>().unwrap();
    let width = button.compute_bounds(&cell).unwrap().width();
    let cell_width = cell.compute_bounds(&cell.parent().unwrap()).unwrap().width();
    assert!((width - cell_width).abs() <= 1., "{context}: the whole cell is a chip target: {width} vs {cell_width}");
}

pub(super) fn assert_leaf_tool_rows(body: &gtk::Widget, view: &layer_ui::ToolSetView, context: &str) {
    assert!(view.groups.is_empty(), "{context}: leaf tools have no sets");
    assert!(!view.subtools.is_empty(), "{context}: leaf choices remain available");
    assert!(descendants::<gtk::Button>(body).iter().all(|button| !button.has_css_class("tool-group")),
        "{context}: no group chips are rendered");
    let rows = descendants::<gtk::Button>(body).into_iter()
        .filter(|button| button.is_mapped() && button.has_css_class("brush-choice")).collect::<Vec<_>>();
    assert_eq!(rows.len(), view.subtools.len(), "{context}: every choice has one row");
    let mut bottom = None;
    for (row, item) in rows.iter().zip(&view.subtools) {
        assert!(item.preview.is_none() && !row.has_css_class("tool-preview-choice")
            && descendants::<gtk::Picture>(row.upcast_ref()).is_empty(), "{context}: tool rows have no preview");
        let label = row.child().unwrap().last_child().unwrap().downcast::<gtk::Label>().unwrap();
        assert_eq!(label.text().as_str(), item.label.as_ref(), "{context}: rows retain shared choice order");
        let parent = row.parent().unwrap();
        let bounds = row.compute_bounds(&parent).unwrap();
        assert!(bounds.x().abs() <= 1. && (bounds.width() - parent.width() as f32).abs() <= 1.,
            "{context}: each tool fills its list width: {bounds:?} vs {}", parent.width());
        let bounds = row.compute_bounds(body).unwrap();
        if let Some(previous) = bottom { assert!(bounds.y() >= previous, "{context}: rows remain vertically ordered"); }
        bottom = Some(bounds.y() + bounds.height());
    }
}

fn tool_drawer_sets(d: &mut Driver) {
    use ToolbarControl::{Command as C, ToolSlot as S};
    for (preset, name, control) in [
        (WorkspacePreset::Painter, "sketch-brush", C { command: CommandId::DrawingBrush }),
        (WorkspacePreset::Painter, "sketch-sculpt", C { command: CommandId::Sculpt }),
        (WorkspacePreset::Illustrator, "paint-brush", C { command: CommandId::DrawingBrush }),
        (WorkspacePreset::Illustrator, "paint-sculpt", C { command: CommandId::Sculpt }),
        (WorkspacePreset::Illustrator, "paint-fill", S { slot: ToolSlotId::Fill }),
        (WorkspacePreset::Illustrator, "paint-gradient", S { slot: ToolSlotId::Gradient }),
        (WorkspacePreset::Photographer, "photo-healing", S { slot: ToolSlotId::Healing }),
        (WorkspacePreset::Photographer, "photo-eraser", C { command: CommandId::Eraser }),
    ] {
        d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
            layout: preset.layout(Platform::Gtk), ..Default::default()
        }) });
        new_photo::ready(&d.w);
        let opener = tool(d, control);
        click(d, &opener);
        new_photo::ready(&d.w);
        if state(&d.w).customization.drawer.is_none() {
            assert!(d.named(&opener).has_css_class("selected-tool"), "{name}: the first tap selects the tool");
            click(d, &opener);
        }
        inventory_drawer_ready(d);
        if name == "sketch-brush" && presentation(d).edge != Edge::Top {
            let body = d.named("tool-drawer");
            let bounds = body.compute_bounds(&d.w.surface).unwrap();
            assert!(bounds.y() <= 12. && bounds.y()+bounds.height() > 12., "the open drawer overlaps the title band");
            let point = [bounds.x()+bounds.width()*0.5,12.];
            let picked = d.w.surface.pick(point[0] as f64,point[1] as f64,gtk::PickFlags::DEFAULT).unwrap();
            assert!(picked == body || picked.is_ancestor(&body), "drawer owns the y12 overlap");
            let presses = Rc::new(Cell::new(0));
            let input = gtk::EventControllerLegacy::new();
            input.set_propagation_phase(gtk::PropagationPhase::Capture);
            input.connect_event(glib::clone!(#[strong] presses, move |_,event| {
                if event.event_type() == gtk::gdk::EventType::ButtonPress { presses.set(presses.get()+1); }
                glib::Propagation::Proceed
            }));
            body.add_controller(input.clone());
            let checkpoint = ui_session(&d.w).engine().checkpoint();
            let point = d.w.surface.compute_point(&d.w.window,&gtk::graphene::Point::new(point[0],point[1])).unwrap();
            d.input.click([point.x(),point.y()]);
            assert_eq!(presses.get(),1,"the actual press reaches the drawer");
            assert_eq!(ui_session(&d.w).engine().checkpoint(),checkpoint);
            assert!(d.w.chrome_contacts.borrow().is_empty());
            body.remove_controller(&input);
        }
        let drawer = state(&d.w).customization.drawer.unwrap();
        let adaptive = matches!(control, C { command: CommandId::DrawingBrush | CommandId::Sculpt });
        if adaptive {
            let sets = if control == (C { command: CommandId::Sculpt }) { Panel::SculptSets } else { Panel::BrushSets };
            assert_eq!(drawer.sets_column, presentation(d).edge != Edge::Top, "{name}");
            assert_eq!(drawer.columns, if drawer.sets_column { vec![vec![sets], vec![Panel::Tools]] } else { vec![vec![Panel::Brushes]] }, "{name}");
        } else { assert!(!drawer.sets_column, "{name}: existing choice drawers keep their arrangement"); }
        for panel in drawer.panels() {
            let body = d.named(&format!("drawer-panel-{panel:?}"));
            let groups = widgets(&body).filter(|widget| widget.has_css_class("tool-group")).count();
            let rows = widgets(&body).filter(|widget| widget.has_css_class("brush-choice")).count();
            for button in descendants::<gtk::Button>(&body).into_iter().filter(|button| button.has_css_class("tool-group") && button.is_mapped()) {
                assert_group_chip(&button, name);
            }
            if matches!(panel, Panel::BrushSets | Panel::SculptSets) {
                assert!(groups > 0 && rows == 0 && descendants::<gtk::Picture>(&body).is_empty(), "{name}: set columns build groups only");
            } else if panel == Panel::Tools && drawer.sets_column {
                assert!(groups == 0 && rows > 0, "{name}: the Tools column builds its list without chips");
            } else if matches!(control, S { slot: ToolSlotId::Fill | ToolSlotId::Gradient | ToolSlotId::Healing }) {
                assert_eq!(groups, 0, "{name}: leaf tools are rows, not sets");
                assert_leaf_tool_rows(&body, drawer.tool_set.as_ref().unwrap(), name);
            } else if adaptive {
                assert!(groups > 0, "{name}: the set choices remain visible");
            }
            for button in descendants::<gtk::Button>(&body).into_iter().filter(|button| button.is_mapped()) {
                let bounds = button.compute_bounds(&body).unwrap();
                assert!(bounds.x() >= -1. && bounds.x() + bounds.width() <= body.width() as f32 + 1., "{name}: a drawer button exceeds its column: {bounds:?}");
            }
        }
        if drawer.sets_column {
            let sets = d.named(&format!("drawer-panel-{:?}", drawer.columns[0][0]));
            let tools = d.named("drawer-panel-Tools");
            let sets = sets.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>().unwrap();
            let tools = tools.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>().unwrap();
            assert_ne!(sets, tools, "{name}: the two columns own separate scrollers");
            let adjustment = sets.vadjustment();
            if adjustment.upper() - adjustment.page_size() > 1. {
                let before = adjustment.value();
                let tools_before = tools.vadjustment().value();
                let point = screen_point(sets.upcast_ref(), &d.w.window, [0.5, 0.5]);
                let direction = if before < adjustment.upper() - adjustment.page_size() { 1 } else { -1 };
                d.input.perform(serde_json::json!([{"point":point},{"wheel":[0,direction]}]));
                until(|| (adjustment.value() - before).abs() > 0.5, "the set column scrolls from native wheel input");
                assert_eq!(tools.vadjustment().value(), tools_before, "{name}: scrolling sets leaves the Tools list in place");
                adjustment.set_value(before);
            }
        }
        scroll_overflow(d);
        capture(d, name);
        if adaptive {
            let view = drawer.tool_set.as_ref().unwrap();
            let body = d.named(&format!("drawer-panel-{:?}", drawer.columns[0][0]));
            let (button, cell) = view.groups.iter().filter(|item| !item.selected && item.enabled).find_map(|item| {
                let row = mapped_label(&body, &item.label).unwrap();
                super::histogram::scroll_to(&row);
                let button = row.ancestor(gtk::Button::static_type()).unwrap();
                let cell = button.parent().unwrap();
                let scroll = button.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>().unwrap();
                let bar = scroll.vscrollbar();
                let bounds = cell.compute_bounds(&d.w.window).unwrap();
                (!bar.is_mapped() || bounds.intersection(&bar.compute_bounds(&d.w.window).unwrap()).is_none()).then_some((button, cell))
            }).expect("an unselected chip has a cell outside the scrollbar");
            let bounds = cell.compute_bounds(&d.w.window).unwrap();
            let point = [bounds.x() + bounds.width() - 2., bounds.y() + bounds.height() * 0.5];
            let hit = d.w.window.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT).unwrap();
            assert!(hit == button || hit.is_ancestor(&button), "the unobscured cell edge belongs to its button");
            d.input.click(point);
            until(|| state(&d.w).customization.drawer.as_ref().and_then(|drawer| drawer.tool_set.as_ref())
                .is_some_and(|updated| updated.subtools != view.subtools), "choosing a set replaces the drawer list");
            inventory_drawer_ready(d);
            let updated = state(&d.w).customization.drawer.unwrap();
            assert_eq!((updated.columns, updated.sets_column), (drawer.columns, drawer.sets_column), "{name}: choosing a set keeps the arrangement");
            capture(d, &format!("{name}-other-set"));
        }
        inventory_close(d);
    }
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_drawers --native-storage; phone viewport and theme supplied externally"]
fn native_phone_drawers() {
    let mut d = setup("art.capycanvas.PhoneDrawers");
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    drawer_selection_colors(&mut d);
    for swapped in [false, true] {
        if swapped { menu(&mut d); click(&mut d, "phone-swap"); }
        for (name, control, panel) in [
            ("brush", ToolbarControl::Command { command: CommandId::DrawingBrush }, if presentation(&d).edge == Edge::Top { Panel::Brushes } else { Panel::BrushSets }),
            ("color", ToolbarControl::Color, Panel::Color),
            ("layers", ToolbarControl::Panel { panel: Panel::Layers }, Panel::Layers),
        ] {
            let opener = tool(&d, control);
            let selected = match control { ToolbarControl::Command { command } => ui_session(&d.w).command(command).selected, _ => true };
            click(&mut d, &opener);
            if !selected { click(&mut d, &opener); }
            until(|| state(&d.w).customization.drawer.is_some(), "phone tool opens its drawer");
            inventory_drawer_ready(&d);
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
                color_drawer(&d);
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
    tool_drawer_sets(&mut d);
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
    for size in [[891, 412], [412, 891]] {
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
                let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
                let gap = if layout.edge == Edge::Top { [(layout.capy.x + layout.capy.width + layout.size.x) * 0.5, layout.capy.y + layout.tile * 0.5] }
                    else { [layout.capy.x + layout.tile * 0.5, (layout.capy.y + layout.capy.height + layout.size.y) * 0.5] };
                let top = if layout.edge == Edge::Top {
                    [layout.chips[0].x + layout.chips[0].width + 4., layout.bars[0].y + layout.tile * 0.5]
                } else { [layout.bars[0].x + layout.tile * 0.5, layout.chips[0].y - 4.] };
                let mut points = if bar == 0 { vec![center(layout.tools[0]), top] }
                    else { vec![center(layout.capy), center(layout.settings), gap] };
                if bar == 0 && layout.edge != Edge::Top {
                    let title = d.named("phone-title-band");
                    let point = [layout.work_area.x + layout.work_area.width*0.5,10.];
                    let picked = d.w.surface.pick(point[0] as f64,point[1] as f64,gtk::PickFlags::DEFAULT).unwrap();
                    assert!(picked == title || picked.is_ancestor(&title), "landscape top input belongs to the title handle");
                    points.push(point);
                } else if layout.edge == Edge::Top { assert!(!d.named("phone-title-band").is_mapped()); }
                for (source, point) in points.into_iter().enumerate() {
                    let from = [origin[0] + point[0], origin[1] + point[1]];
                    let delta = [if origin[0] + size[0] as f32 * 0.5 > monitor.width() as f32 * 0.5 { -60. } else { 60. },
                        if origin[1] + size[1] as f32 * 0.5 > monitor.height() as f32 * 0.5 { -30. } else { 30. }];
                    let to = [from[0] + delta[0], from[1] + delta[1]];
                    let before = layer_ui::durable_layout(&state(&d.w).workspace.layout);
                    let checkpoint = ui_session(&d.w).engine().checkpoint();
                    let selected = state(&d.w).layer_tools.tool;
                    d.input.perform(serde_json::json!([contact(device,"down",from),
                        contact(device,"move",[from[0]+delta[0]*0.3,from[1]+delta[1]*0.3]),
                        contact(device,"move",to),contact(device,"up",to)]));
                    assert!(d.w.chrome_contacts.borrow().is_empty(), "compositor handoff retires chrome capture before any pointer probe");
                    assert_eq!(state(&d.w).layer_tools.tool, selected);
                    assert!(!state(&d.w).phone.menu && !widgets(d.w.window.upcast_ref()).any(|widget| widget.is_mapped() && widget.is::<gtk::Popover>()));
                    let probe = [origin[0] + size[0] as f32 * 0.5, origin[1] + size[1] as f32 * 0.5];
                    observed.set(None);
                    d.input.perform(serde_json::json!([{"point":[probe[0]-1.,probe[1]]},{"point":probe}]));
                    let local = observed.get().expect("pointer inside moved phone window");
                    let moved = [probe[0] - local[0], probe[1] - local[1]];
                    assert!((moved[0] - origin[0]) * delta[0].signum() > 30.
                        && (moved[1] - origin[1]) * delta[1].signum() > 10.,
                        "{size:?}/{device}/bar {bar}/source {source}: actual origin {origin:?} -> {moved:?}, expected delta {delta:?}");
                    assert_eq!(layer_ui::durable_layout(&state(&d.w).workspace.layout), before);
                    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
                    assert!(d.w.chrome_contacts.borrow().is_empty());
                    origin = moved;
                }
            }
        }
        if presentation(&d).edge != Edge::Top {
            d.w.dispatch(UiAction::Phone { action: PhoneAction::Capy });
            until(|| state(&d.w).phone.menu && d.w.drawer.placement().is_some(), "the landscape menu opens for overlap input");
            let menu = d.named("phone-menu");
            let bounds = menu.compute_bounds(&d.w.surface).unwrap();
            assert!(bounds.y() <= 12. && bounds.y()+bounds.height() > 12., "the menu overlaps the title band");
            let local = [bounds.x()+bounds.width()*0.5,12.];
            let picked = d.w.surface.pick(local[0] as f64,local[1] as f64,gtk::PickFlags::DEFAULT).unwrap();
            assert!(picked == menu || picked.is_ancestor(&menu),"the menu owns the overlapping press");
            let checkpoint = ui_session(&d.w).engine().checkpoint();
            let from = [origin[0]+local[0],origin[1]+local[1]];
            let to = [from[0]+30.,from[1]];
            d.input.perform(serde_json::json!([contact("mouse","down",from),contact("mouse","move",to),contact("mouse","up",to)]));
            assert!(state(&d.w).phone.menu,"dragging the root menu does not hand off window movement");
            let probe = [origin[0]+size[0] as f32*0.5,origin[1]+size[1] as f32*0.5];
            observed.set(None);
            d.input.perform(serde_json::json!([{"point":[probe[0]-1.,probe[1]]},{"point":probe}]));
            let local = observed.get().expect("pointer inside the menu overlap window");
            assert!((probe[0]-local[0]-origin[0]).abs() <= 1. && (probe[1]-local[1]-origin[1]).abs() <= 1.,
                "menu input leaves the actual window origin unchanged");
            assert_eq!(ui_session(&d.w).engine().checkpoint(),checkpoint);
            assert!(d.w.chrome_contacts.borrow().is_empty());
            d.w.dispatch(UiAction::Phone { action: PhoneAction::Capy });
            until(|| !state(&d.w).phone.menu,"close the overlap menu before changing orientation");
        }
    }
    d.finish();
}

fn menu_pages(d: &mut Driver) {
    let swapped = state(&d.w).phone.swapped;
    let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    let main = stack.visible_child_name().unwrap();
    let initial = d.w.drawer.placement().as_ref().unwrap().bounds;
    assert_eq!(main.as_str(), "main");
    menu_bounds(d, initial);
    menu_semantics(d, &stack);
    let prefs = d.named("phone-preferences");
    let search = d.named("phone-search");
    inside(d, &prefs);
    inside(d, &search);
    assert!(prefs.tooltip_text().is_some_and(|label| !label.is_empty()));
    assert!(gtk::test_accessible_has_property(&prefs, gtk::AccessibleProperty::Label));
    assert!(search.is::<gtk::Button>(), "Search is an icon-strip button");
    let submenus = ui_session(&d.w).phone_menu().unwrap().list.sections.iter().flatten()
        .filter(|item| !item.sections.is_empty()).take(2).map(|item| item.label.clone()).collect::<Vec<_>>();
    assert_eq!(submenus.len(), 2);
    for (index, label) in submenus.into_iter().enumerate() {
        let row = widgets(&d.named("phone-menu-list")).find(|widget|
            widget.is_mapped() && widget.css_name() == "modelbutton"
                && menu_text(&widget) == label).unwrap();
        d.click(&row);
        until(|| stack.visible_child_name().as_deref() == Some(menu_page_name(d, &label).as_str())
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
    let window_label = ApplicationMenu::Window.localized_label(&d.w.localization());
    d.click_label(&window_label);
    let window_page = menu_page_name(d, &window_label);
    until(|| stack.visible_child_name().as_deref() == Some(window_page.as_str()) && !stack.is_transition_running(),
        "Window opens inside the one menu list");
    d.click_label("Workspaces");
    let workspaces_page = menu_page_name(d, "Workspaces");
    until(|| stack.visible_child_name().as_deref() == Some(workspaces_page.as_str()) && !stack.is_transition_running(),
        "Workspaces opens inside Window");
    submenu(d, &stack, &format!("workspaces-{swapped}"), initial);
    for page in [window_page.as_str(), "main"] {
        click(d, "phone-menu-back");
        until(|| stack.visible_child_name().as_deref() == Some(page) && !stack.is_transition_running(),
            "Workspaces Back follows its native menu hierarchy");
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
    assert!(d.w.visible_dialog().as_ref() != Some(&d.w.preferences.dialog), "press capture does not activate Preferences");
    capture_current(d,&format!("preferences-pressed-{swapped}"));
    assert!(d.w.visible_dialog().as_ref() != Some(&d.w.preferences.dialog), "Preferences opens on release");
    d.input.perform(serde_json::json!([contact("mouse","up",point)]));
    until(|| d.w.visible_dialog().as_ref() == Some(&d.w.preferences.dialog), "Preferences gear opens Settings");
    crate::page_dialog::close_dialog(&d.w.preferences.dialog);
    until(|| d.w.visible_dialog().as_ref() != Some(&d.w.preferences.dialog)
        && !d.w.pages.root.is_transition_running() && !d.w.pages.root.is_mapped(), "Settings closes");
    if !state(&d.w).phone.menu { menu(d); }
    let retired = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    d.input.key(0xff1b);
    until(|| !state(&d.w).phone.menu && d.w.drawer.placement().is_none(), "Escape closes the menu before replacement");
    menu(d);
    let current = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    assert_ne!(retired, current);
    retired.set_visible_child_name("drawings");
    d.w.surface.queue_allocate();
    capture(d, &format!("retired-menu-stack-{swapped}"));
    assert_eq!(current.visible_child_name().as_deref(), Some("main"));
    menu_semantics(d, &current);
    assert!(d.named("phone-search").is_mapped(), "the icon strip survives navigation");
    menu_bounds(d, initial);
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
        && widget.css_name()=="modelbutton" && menu_text(&widget)==label.as_ref()).unwrap();
    d.click(&row);
    until(|| camera_stack.visible_child_name().as_deref()==Some(menu_page_name(d, &label).as_str()) && !camera_stack.is_transition_running(),
        "File opens at free zoom");
    let bounds = d.w.drawer.placement().as_ref().unwrap().bounds;
    submenu(d,&camera_stack,&format!("camera-close-file-{swapped}"),bounds);
    d.w.dispatch(UiAction::Invoke { command: CommandId::ZoomIn });
    until(|| !state(&d.w).phone.menu && d.w.drawer.placement().is_none(), "camera-only command closes the menu");
    assert!(state(&d.w).camera.zoom > zoom);
    assert_eq!([CommandId::FitCanvas,CommandId::ActualPixels].map(|command| ui_session(&d.w).command(command).selected), selected);
    menu(d);
    let reopened = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    capture(d,&format!("camera-reopened-root-{swapped}"));
    assert_eq!(reopened.visible_child_name().as_deref(),Some("main"));
    menu_semantics(d,&reopened);
    assert!(d.named("phone-search").is_mapped(), "the icon strip survives navigation");
    menu_bounds(d,d.w.drawer.placement().as_ref().unwrap().bounds);
    d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    new_photo::ready(&d.w);
    menu(d);
}

fn submenu(d: &mut Driver, stack: &gtk::Stack, name: &str, initial: Bounds) {
    capture(d, name);
    menu_bounds(d, initial);
    menu_semantics(d, stack);
    let scroll = d.named("phone-menu-scroll").downcast::<gtk::ScrolledWindow>().unwrap();
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

fn drawing_selector(d: &Driver) -> DrawingSelector {
    let gpu = d.w.gpu.borrow();
    let session = &gpu.as_ref().unwrap().session;
    let labels = d.w.documents.model.borrow().labels(&session.state().document_file, |parked| &parked.canvas.session.state().document_file, &d.w.localization());
    DrawingSelector::new(&labels, d.w.documents.selected(), &session.command(CommandId::NewDocument), &d.w.localization())
}

fn drawings_ready(d: &Driver) -> bool {
    if !state(&d.w).phone.menu || d.w.pages.root.is_visible() { return false; }
    let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    stack.is_mapped() && stack.visible_child_name().as_deref() == Some("drawings") && !stack.is_transition_running()
        && stack.visible_child().and_then(|page| find_named(&page, "phone-menu-back"))
            .is_some_and(|back| back.is_mapped() && back.width() > 0 && back.height() > 0)
}

fn menu_text(widget: &gtk::Widget) -> String {
    widgets(widget).find_map(|widget| widget.downcast::<gtk::Label>().ok()).unwrap().text().to_string()
}
fn menu_semantics(d: &Driver, stack: &gtk::Stack) {
    let model = ui_session(&d.w).phone_menu().unwrap().view(&drawing_selector(d), Platform::Gtk, &d.w.localization());
    let name = stack.visible_child_name().unwrap();
    let expected = model.pages.iter().find(|page| page.id.name() == name).unwrap();
    let child = stack.visible_child().unwrap();
    assert_eq!(stack.page(&child).title().as_deref(), Some(expected.title.as_str()));
    let mut rows = widgets(&child).filter(|widget| widget.is_mapped() && widget.css_name() == "modelbutton");
    if expected.parent.is_some() {
        let back = rows.next().unwrap();
        assert_eq!(back.widget_name(), "phone-menu-back");
        assert_eq!(menu_text(&back), expected.title);
    }
    let rows = rows.collect::<Vec<_>>();
    let items = expected.sections.iter().flatten().filter(|item| !item.is_heading).collect::<Vec<_>>();
    assert_eq!(rows.len(), items.len(), "{name}: the whole native page matches the shared menu");
    for (row, item) in rows.iter().zip(items) {
        assert_eq!(menu_text(row), item.label, "{name}: actual menu contents");
        assert_eq!(row.is_sensitive(), item.enabled);
        assert_eq!(row.accessible_role(), if item.selected.is_some() { gtk::AccessibleRole::MenuItemCheckbox } else { gtk::AccessibleRole::MenuItem });
        assert!(gtk::test_accessible_has_property(row, gtk::AccessibleProperty::HasPopup));
    }
    menu_separator(d);
}

fn menu_bounds(d: &Driver, initial: Bounds) {
    assert_eq!(d.w.drawer.placement().as_ref().unwrap().bounds, initial,
        "navigation preserves the original menu shell");
    for name in ["phone-menu", "phone-menu-scroll", "phone-preferences", "phone-search", "phone-finger", "phone-zen", "phone-swap"] {
        let widget = d.named(name);
        assert!(widget.is_mapped());
        let bounds = widget.compute_bounds(&d.w.surface).unwrap();
        assert!(bounds.x() >= initial.x - 1. && bounds.y() >= initial.y - 1.
            && bounds.x() + bounds.width() <= initial.x + initial.width + 1.
            && bounds.y() + bounds.height() <= initial.y + initial.height + 1., "{name}: {bounds:?}, {initial:?}");
    }
}

fn menu_separator(d: &Driver) {
    let placement = d.w.drawer.placement().as_ref().unwrap().clone();
    let expected = presentation(d).menu_geometry(&placement, menu_row_height(d)).separator;
    let separator = d.named("phone-menu-separator").downcast::<gtk::Separator>().unwrap();
    inside(d, separator.upcast_ref());
    let actual = separator.compute_bounds(&d.named("phone-menu")).unwrap();
    assert!([actual.x() - expected.x, actual.y() - expected.y, actual.width() - expected.width,
        actual.height() - expected.height].into_iter().all(|delta| delta.abs() <= 1.));
    assert_eq!(separator.orientation(), if expected.width > expected.height { gtk::Orientation::Horizontal } else { gtk::Orientation::Vertical });
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_menu_strip --native-storage; viewport/theme external"]
fn native_phone_menu_strip() {
    let mut d = setup("art.capycanvas.PhoneMenuStrip");
    for swapped in [false, true] {
        if state(&d.w).phone.swapped != swapped { d.w.dispatch(UiAction::Phone { action: PhoneAction::SwapSides }); }
        menu(&mut d);
        if let Some(directory) = std::env::var_os("LAYER_NATIVE_CAPTURE_DIR") {
            let placement = d.w.drawer.placement().as_ref().unwrap().clone();
            let view = presentation(&d).menu_geometry(&placement, menu_row_height(&d));
            std::fs::write(std::path::PathBuf::from(directory).join(format!("menu-strip-{swapped}.json")),
                serde_json::to_vec(&serde_json::json!({"menu": placement.bounds, "strip": view.strip, "separator": view.separator,
                    "row_height": menu_row_height(&d), "icon": view.icon})).unwrap()).unwrap();
        }
        let pages = ui_session(&d.w).phone_menu().unwrap().view(&drawing_selector(&d), Platform::Gtk, &d.w.localization()).pages;
        let bounds = d.w.drawer.placement().as_ref().unwrap().bounds;
        capture_menu_pages(&mut d, &pages, &PhoneMenuRoute::default(), bounds, swapped);
    }
    d.finish();
}

fn capture_menu_pages(d: &mut Driver, pages: &[PhoneMenuPageView], route: &PhoneMenuRoute, bounds: Bounds, swapped: bool) {
    let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    let page = pages.iter().find(|page| page.id == *route).unwrap();
    until(|| state(&d.w).phone.menu_navigation.current() == *route && !stack.is_transition_running(), "the typed menu page is ready");
    menu_bounds(d, bounds);
    capture(d, &format!("menu-{}{}", route.name(), if swapped { "-swapped" } else { "" }));
    for row in page.sections.iter().flatten().filter(|row| row.target.is_some()) {
        let target = row.target.as_ref().unwrap();
        let widget = widgets(&stack.visible_child().unwrap()).find(|widget|
            widget.has_css_class("submenu-row") && menu_text(widget) == row.label).unwrap();
        click_widget(d, &widget);
        until(|| state(&d.w).phone.menu_navigation.current() == *target && !stack.is_transition_running(), "the typed submenu opens by native input");
        capture_menu_pages(d, pages, target, bounds, swapped);
        click(d, "phone-menu-back");
        until(|| state(&d.w).phone.menu_navigation.current() == *route && !stack.is_transition_running(), "Back retains the typed parent and root bounds");
        menu_bounds(d, bounds);
    }
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_menu_live_measurement --native-storage; landscape phone viewport/theme external"]
fn native_phone_menu_live_measurement() {
    let mut d = setup("art.capycanvas.PhoneMenuLiveMeasurement");
    assert!(d.w.surface.width() > d.w.surface.height());
    ui_session_mut(&d.w).name_new_drawing("Small");
    d.w.refresh(regions::DOCUMENT);
    menu(&mut d);
    for (tag, title) in [("long", "A drawing with a long name that needs more room in the open menu"), ("short", "Small")] {
        ui_session_mut(&d.w).name_new_drawing(title);
        d.w.refresh(regions::DOCUMENT);
        until(|| menu_text(&d.named("phone-drawing")) == drawing_selector(&d).label,
            "the open menu publishes the new drawing label");
        drop(idle_paint(&d.w.window));
        let live = d.w.drawer.placement().as_ref().unwrap().bounds;
        capture(&mut d, &format!("menu-live-{tag}"));
        d.input.key(0xff1b);
        until(|| !state(&d.w).phone.menu, "the menu closes before measuring a fresh opening");
        menu(&mut d);
        let fresh = d.w.drawer.placement().as_ref().unwrap().bounds;
        capture(&mut d, &format!("menu-fresh-{tag}"));
        assert!((live.width - fresh.width).abs() <= 1. && (live.height - fresh.height).abs() <= 1.,
            "{tag}: an open menu remeasures its content like a fresh menu: {live:?} vs {fresh:?}");
    }
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_menu_feedback --native-storage; viewport/theme external"]
fn native_phone_menu_feedback() {
    let mut d = setup("art.capycanvas.PhoneMenuFeedback");
    let first = d.w.documents.selected();
    d.w.dispatch(UiAction::Invoke { command: CommandId::AddLayer });
    until(|| !ui_session(&d.w).engine().has_pending_document_edits(), "the first drawing is modified");
    d.w.documents.enqueue(&d.w, (new_drawing_at(128, 96, layer_core::color::SampleDepth::U8), None));
    until(|| d.w.documents.len() == 2 && !d.w.documents.changing.get(), "a second drawing opens");
    new_photo::ready(&d.w);
    let second = d.w.documents.selected();
    menu(&mut d);
    let caption = d.named("phone-drawing");
    let root_bounds = d.w.drawer.placement().as_ref().unwrap().bounds;
    assert_eq!(caption.css_name(), "modelbutton");
    assert!(caption.is_sensitive());
    assert!(!gtk::test_accessible_has_property(&caption, gtk::AccessibleProperty::KeyShortcuts), "the count is no keyboard shortcut");
    assert!(widgets(&caption).filter_map(|w| w.downcast::<gtk::Label>().ok()).any(|label| label.widget_name() == "menu-detail" && label.text() == "2"));
    assert!(!d.w.system_status.clock.is_ancestor(&d.named("phone-menu")) && !d.w.system_status.battery.is_ancestor(&d.named("phone-menu")));
    let file = widgets(&d.named("phone-menu-list")).find(|row| row.css_name() == "modelbutton" && menu_text(&row) == ApplicationMenu::File.localized_label(&d.w.localization()).as_ref()).unwrap();
    assert_eq!(caption.height(), file.height(), "the drawing opener has native menu-row height");
    let row_height = file.height();
    let text_start = |row: &gtk::Widget| {
        let label = widgets(row).filter_map(|w| w.downcast::<gtk::Label>().ok()).find(|label| label.css_name() == "label").unwrap();
        label.compute_bounds(&d.w.surface).unwrap().x() + label.layout_offsets().0 as f32
    };
    assert!((text_start(&caption) - text_start(&file)).abs() <= 1., "the drawing label keeps the native page inset and check gutter");
    capture(&mut d, "feedback-root-two");
    for name in ["phone-finger", "phone-swap"] {
        let row = d.named(name); let before = row.property::<bool>("active");
        for selected in [!before, before] {
            click(&mut d, name);
            until(|| state(&d.w).phone.menu && d.named(name).property::<bool>("active") == selected, "the toggle updates without closing");
            assert_eq!(row, d.named(name), "the native check row is updated in place");
        }
    }
    click(&mut d, "phone-drawing");
    let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    until(|| drawings_ready(&d), "Drawings slides into the navigation pane");
    menu_bounds(&d, root_bounds);
    menu_semantics(&d, &stack);
    assert!(!d.w.popovers.borrow().iter().filter_map(|p| p.upgrade()).any(|p| p.is_mapped()), "Drawings has no floating selector");
    for id in [first, second] {
        let row = d.named(&format!("phone-drawing-{id}"));
        assert_eq!(row.css_name(), "modelbutton");
        assert_eq!(row.height(), row_height, "drawing rows retain native menu height beside their close controls");
        assert_eq!(row.state_flags().contains(gtk::StateFlags::CHECKED), id == second);
        inside(&d, &row); inside(&d, &d.named(&format!("phone-drawing-close-{id}")));
    }
    assert!(d.named("phone-search").is_mapped(), "the icon strip stays visible on Drawings");
    capture(&mut d, "feedback-drawings-two");
    click(&mut d, &format!("phone-drawing-{first}"));
    until(|| d.w.documents.selected() == first && !state(&d.w).phone.menu && !d.w.documents.changing.get(), "a drawing selection switches and closes");
    new_photo::ready(&d.w);
    d.w.dispatch(UiAction::Invoke { command: CommandId::Drawings });
    until(|| drawings_ready(&d), "the Drawings command opens the phone page");
    menu_bounds(&d, root_bounds);
    let current = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    assert_ne!(stack, current);
    let old_back = widgets(&stack.visible_child().unwrap()).find(|row| row.css_name() == "modelbutton").unwrap();
    old_back.emit_by_name::<()>("clicked", &[]);
    assert_eq!(current.visible_child_name().as_deref(), Some("drawings"), "a retained page owns its external opener and cannot change current navigation");
    assert!(d.named(&format!("phone-drawing-{first}")).state_flags().contains(gtk::StateFlags::CHECKED));
    capture(&mut d, "feedback-drawings-current");
    d.w.dispatch(UiAction::Phone { action: PhoneAction::Drawing { id: second, close: false } });
    let choice = if ui_session(&d.w).finger_drawing() { 2 } else { 1 };
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::FingerDrawing, value: PreferenceValue::Choice(choice) } });
    until(|| d.w.documents.selected() == second && !d.w.documents.changing.get() && !d.w.servicing.get(), "selection survives queued settings persistence");
    new_photo::ready(&d.w);
    d.w.dispatch(UiAction::Invoke { command: CommandId::Drawings });
    until(|| drawings_ready(&d), "Drawings reopens after queued selection");
    menu_bounds(&d, root_bounds);
    click(&mut d, "phone-menu-back");
    until(|| {
        let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
        stack.visible_child_name().as_deref() == Some("main") && !stack.is_transition_running() && d.named("phone-drawing").is_mapped()
    }, "Drawings Back returns to the menu");
    menu_bounds(&d, root_bounds);
    click(&mut d, "phone-drawing");
    until(|| drawings_ready(&d), "Drawings is ready before New");
    click(&mut d, "phone-new-drawing");
    until(|| d.w.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "new-drawing-dialog"), "New drawing keeps its normal editor");
    crate::page_dialog::close_dialog(&d.w.visible_dialog().unwrap());
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running() && !d.w.servicing.get(), "New drawing cancels and releases modal input");
    d.w.dispatch(UiAction::Invoke { command: CommandId::Drawings });
    until(|| drawings_ready(&d), "the close control is visible");
    click(&mut d, &format!("phone-drawing-close-{first}"));
    until(|| d.w.window.visible_dialog().is_some_and(|dialog| dialog.is::<adw::AlertDialog>()), "closing an unsaved drawing asks to save");
    new_photo::response(&d.w, "discard");
    until(|| d.w.documents.len() == 1 && !d.w.documents.changing.get(), "discard closes only that drawing");
    new_photo::ready(&d.w); menu(&mut d);
    assert!(!widgets(&d.named("phone-drawing")).filter_map(|w| w.downcast::<gtk::Label>().ok()).any(|label| label.widget_name() == "menu-detail" && label.is_mapped()));
    click(&mut d, "phone-zen");
    until(|| state(&d.w).phone.hidden && !state(&d.w).phone.menu, "Zen closes the menu and hides bars");
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_layers_live_height --native-storage; 412x891 light"]
fn native_phone_layers_live_height() {
    fn settled(d: &Driver) {
        until(|| {
            if ui_session(&d.w).engine().has_pending_document_edits() { return false; }
            let Some(placement) = d.w.drawer.placement() else { return false; };
            let Some(layers) = d.w.drawer.layers() else { return false; };
            let Some(stack) = layers.root.ancestor(gtk::Stack::static_type()).and_downcast::<gtk::Stack>() else { return false; };
            if !stack.is_mapped() || stack.is_transition_running() { return false; }
            let view = state(&d.w);
            let properties = view.customization.drawer.as_ref().is_some_and(|drawer| drawer.layer_properties == Some(true));
            let measurement = layers.scroll_measurement(placement.bounds.width as i32);
            let properties_height = d.w.drawer.effects().unwrap().properties.measure(gtk::Orientation::Vertical, placement.bounds.width as i32).1 as f32;
            let expected = LayerDrawerSizing::presented_pages(measurement, view.layer_tools.presented_rows(&view.layers),
                d.w.surface.height() as f32, properties_height, Some(&presentation(d)))[usize::from(properties)];
            if (placement.bounds.height - expected).abs() > 1. { return false; }
            if properties { return true; }
            let Some(id) = view.layer_tools.editing_layer.as_ref().map(|row| row.id) else { return false; };
            find_named(layers.list.upcast_ref(), &format!("art-layer-{id}")).filter(|row| row.is_mapped())
                .and_then(|row| row.compute_bounds(&layers.list)).is_some_and(|b| b.y() >= -1. && b.y() + b.height() <= layers.list.height() as f32 + 1.)
        }, "the open drawer follows shared sizing and reveals its selected row");
    }
    let mut d = setup("art.capycanvas.PhoneLayersHeight");
    gtk::Settings::default().unwrap().set_gtk_enable_animations(true);
    assert_eq!([d.w.surface.width(), d.w.surface.height()], [412, 891]);
    assert_eq!(state(&d.w).theme, Theme::Light);
    let name = tool(&d, ToolbarControl::Panel { panel: Panel::Layers });
    click(&mut d, &name);
    until(|| d.w.drawer.layers().is_some(), "Layers opens once");
    let anchor = state(&d.w).customization.drawer.unwrap().anchor;
    settled(&d);
    assert_eq!(state(&d.w).layers.len(), 2);
    capture(&mut d, "layers-live-2");
    let mut was_capped = false;
    let mut grew_while_capped = false;
    for count in 3..=24 {
        if was_capped {
            let adjustment = d.w.drawer.layers().unwrap().list.vadjustment();
            adjustment.set_value((adjustment.upper() - adjustment.page_size()).max(adjustment.lower()));
        }
        click(&mut d, "layer-add-layer-symbolic");
        until(|| state(&d.w).layers.len() == count, "the native action adds a layer while the drawer stays open");
        settled(&d);
        assert_eq!(state(&d.w).customization.drawer.unwrap().anchor, anchor);
        if [5, 12].contains(&count) { capture(&mut d, &format!("layers-live-{count}")); }
        let capped = (d.w.drawer.placement().unwrap().bounds.height - presentation(&d).work_area.height).abs() <= 1.;
        grew_while_capped |= was_capped && capped;
        was_capped = capped;
        if grew_while_capped && count >= 12 { break; }
    }
    assert!(grew_while_capped, "a new layer remains visible after height reaches the cap");
    let layers = d.w.drawer.layers().unwrap();
    assert!(layers.list.vadjustment().upper() > layers.list.vadjustment().page_size());
    capture(&mut d, "layers-live-capped");
    while state(&d.w).layers.len() > 5 {
        let count = state(&d.w).layers.len();
        click(&mut d, "delete-selected-layers");
        until(|| state(&d.w).layers.len() == count - 1, "the native action removes a layer");
        settled(&d);
    }
    assert!(d.w.drawer.placement().unwrap().bounds.height < presentation(&d).work_area.height);
    click(&mut d, "layer-folder-symbolic");
    until(|| state(&d.w).layer_tools.editing_layer.is_some_and(|layer| layer.group), "the native action creates a group");
    let group = state(&d.w).layer_tools.editing_layer.unwrap().id;
    click(&mut d, "layer-add-layer-symbolic");
    settled(&d);
    let expanded = d.w.drawer.placement().unwrap().bounds.height;
    d.w.dispatch(UiAction::Layer { action: LayerAction::Collapse { id: group } });
    settled(&d);
    assert!(d.w.drawer.placement().unwrap().bounds.height < expanded);
    d.w.dispatch(UiAction::Layer { action: LayerAction::Collapse { id: group } });
    settled(&d);
    assert_eq!(d.w.drawer.placement().unwrap().bounds.height, expanded);
    let heights = Rc::new(RefCell::new(Vec::new()));
    let clock = d.w.surface.frame_clock().unwrap();
    let w = d.w.clone();
    let handler = clock.connect_after_paint(glib::clone!(#[weak] w, #[strong] heights, move |_| {
        if let Some(placement) = w.drawer.placement() { heights.borrow_mut().push(placement.bounds.height); }
    }));
    click(&mut d, "layer-properties-disclosure");
    until(|| state(&d.w).customization.drawer.is_some_and(|drawer| drawer.layer_properties == Some(true)), "Properties opens in the same drawer");
    settled(&d);
    let properties_height = d.w.drawer.placement().unwrap().bounds.height;
    clock.disconnect(handler);
    assert!((expanded - properties_height).abs() > 2., "the fixture changes page height");
    assert!(heights.borrow().iter().any(|height| *height > expanded.min(properties_height) + 1. && *height < expanded.max(properties_height) - 1.), "the drawer shell follows the page slide through intermediate heights");
    click(&mut d, "layer-properties-disclosure");
    until(|| state(&d.w).customization.drawer.is_some_and(|drawer| drawer.layer_properties == Some(false)), "the footer returns to Layers");
    settled(&d);
    capture(&mut d, "layers-live-return");
    d.input.key(0xff1b);
    d.finish();
}

fn menu_layers(d: &mut Driver) {
    fn layers_path(sections: &[Vec<ContextMenuItem>]) -> Option<Vec<String>> {
        sections.iter().flatten().find_map(|item| {
            if matches!(item.action, Some(UiAction::Phone { action: PhoneAction::Panel { panel: Panel::Layers } })) { Some(vec![item.label.clone()]) }
            else { layers_path(&item.sections).map(|mut path| { path.insert(0, item.label.clone()); path }) }
        })
    }
    let path = layers_path(&ui_session(&d.w).phone_menu().unwrap().list.sections).unwrap();
    for label in path {
        let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
        until(|| !stack.is_transition_running() && widgets(stack.upcast_ref()).any(|row| row.is_mapped() && row.css_name() == "modelbutton" && menu_text(&row) == label), "Window panel action is mapped after its page slide");
        let row = widgets(stack.upcast_ref()).find(|row| row.is_mapped() && row.css_name() == "modelbutton" && menu_text(&row) == label).unwrap();
        assert!(row.grab_focus());
        let scroll = row.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>().unwrap();
        until(|| row.compute_bounds(&scroll).is_some_and(|b| b.y() >= -1. && b.y() + b.height() <= scroll.height() as f32 + 1.), "the Window action scrolls into view");
        d.click(&row);
    }
    until(|| state(&d.w).customization.drawer.as_ref().is_some_and(|drawer| drawer.anchor == DrawerAnchor::Phone { anchor: PhoneDrawerAnchor::Capy }), "Window Layers opens from Capy");
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_control_feedback --native-storage; 412x891 light"]
fn native_phone_control_feedback() {
    fn close(d: &mut Driver) {
        d.input.key(0xff1b);
        until(|| state(&d.w).customization.drawer.is_none() && d.w.drawer.is_closed() && !state(&d.w).phone.menu, "the phone surface closes");
    }
    let mut d = setup("art.capycanvas.PhoneControlFeedback");
    assert_eq!([d.w.surface.width(), d.w.surface.height()], [412, 891]);
    for command in [CommandId::DrawingBrush, CommandId::Fill, CommandId::Select] {
        d.w.dispatch(UiAction::Invoke { command });
        click(&mut d, "phone-tool-settings");
        until(|| state(&d.w).customization.drawer.as_ref().is_some_and(|drawer| drawer.columns == vec![vec![Panel::ToolSettings]]), "the fixed tile opens current Tool settings alone");
        inventory_drawer_ready(&d);
        let place = d.w.drawer.placement().unwrap();
        let button = d.named("phone-tool-settings").compute_bounds(&d.w.surface).unwrap();
        assert!([place.anchor.x - button.x(), place.anchor.y - button.y(), place.anchor.width - button.width(), place.anchor.height - button.height()]
            .into_iter().all(|difference| difference.abs() <= 0.5), "the bridge starts on the allocated Tool settings tile");
        assert!(place.connection().is_some());
        inside(&d, &d.named("drawer-panel-ToolSettings"));
        capture(&mut d, &format!("fixed-settings-{command:?}").to_lowercase());
        close(&mut d);
    }
    d.w.dispatch(UiAction::Invoke { command: CommandId::DrawingBrush });
    let brush = tool(&d, ToolbarControl::Command { command: CommandId::DrawingBrush });
    click(&mut d, &brush);
    until(|| state(&d.w).customization.drawer.is_some(), "Brush opens its list");
    inventory_drawer_ready(&d);
    assert!(!state(&d.w).customization.drawer.unwrap().panels().any(|p| p == Panel::ToolSettings));
    capture(&mut d, "brush-without-settings");
    close(&mut d);
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for material in [0, 2] {
            d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Transparency, value: PreferenceValue::Choice(material) } });
            let tag = format!("material-{theme:?}-{material}").to_lowercase();
            capture(&mut d, &format!("{tag}-closed"));
            click(&mut d, &brush);
            until(|| state(&d.w).customization.drawer.is_some(), "tool drawer opens for material comparison");
            inventory_drawer_ready(&d);
            capture(&mut d, &format!("{tag}-tool"));
            assert!(!d.w.zen_capy.has_css_class("drawer-open"));
            assert!(!d.w.zen_capy.state_flags().intersects(gtk::StateFlags::PRELIGHT | gtk::StateFlags::ACTIVE));
            close(&mut d);
            menu(&mut d);
            capture(&mut d, &format!("{tag}-menu"));
            assert!(d.w.zen_capy.has_css_class("drawer-open"));
            close(&mut d);
            assert!(!d.w.zen_capy.has_css_class("drawer-open"), "closing a menu restores Capy's chip");
            menu(&mut d);
            menu_layers(&mut d);
            inventory_drawer_ready(&d);
            assert!(d.w.zen_capy.has_css_class("drawer-open"));
            capture(&mut d, &format!("{tag}-capy-layers"));
            close(&mut d);
        }
    }
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_toolbar_slider_stretch --native-storage; 412x891 or 1600x1000 light"]
fn native_toolbar_slider_stretch() {
    let mut d = setup("art.capycanvas.ToolbarSliderStretch");
    let phone = state(&d.w).phone_presentation([d.w.surface.width() as f32, d.w.surface.height() as f32]);
    let name = if phone.is_some() { "component-slider-size".to_string() } else {
        let (panel, id) = state(&d.w).workspace.layout.panels.iter().find_map(|panel| panel.tiles().iter()
            .find(|tile| tile.control == ToolbarControl::BrushSizeSlider).map(|tile| (panel.id, tile.id))).unwrap();
        d.w.dispatch(UiAction::MovePanel { panel, target: DockTarget::CompactEdge { edge: Edge::Top, alignment: EdgeAlignment::Center },
            viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32] });
        format!("component-slider-{id}")
    };
    until(|| widgets(d.w.window.upcast_ref()).any(|widget| widget.is_mapped() && widget.widget_name() == name
        && widget.width() > widget.height()), "the horizontal brush slider is allocated");
    let scale = d.named(&name).downcast::<gtk::Scale>().unwrap();
    let component = scale.parent().unwrap();
    let band = component.compute_bounds(&d.w.window).unwrap();
    let transition = phone.as_ref().map_or(4. * band.height(), |p| p.slider_stretch_distance());
    let measure = || {
        let range = scale.range_rect();
        let (start, end) = scale.slider_range();
        let half = (end - start) as f32 * 0.5;
        let to_window = |x| scale.compute_point(&d.w.window, &gtk::graphene::Point::new(x, 0.)).unwrap().x();
        ToolbarSliderStretch { track: [to_window(range.x() as f32 + half), to_window((range.x() + range.width()) as f32 - half)],
            band: [band.y(), band.y() + band.height()], extent: d.w.window.width() as f32, transition }
    };
    let mut geometry = measure();
    let x = geometry.track[0] + (geometry.track[1] - geometry.track[0]) * 0.35;
    let y = band.y() + band.height() * 0.5;
    let far_y = if phone.is_some() { band.y() - transition } else { band.y() + band.height() + transition };
    let points = [[x, y], [x + 8., y], [x + 8., far_y], [1., far_y], [geometry.extent - 1., far_y], [x + 8., y]]
        .map(|point| point.map(|v| (v * 256.).round() / 256.));
    for (i, point) in points.into_iter().enumerate() {
        d.input.perform(serde_json::json!([contact("mouse", if i == 0 { "down" } else { "move" }, point)]));
        if i == 0 { geometry = measure(); }
        let expected = toolbar_slider_stretch(geometry, point);
        if i > 0 {
            let value = ToolbarControl::BrushSizeSlider.slider().unwrap().numeric().resolve(0., NumericOperation::Position { position: expected as f64 }).unwrap();
            let expected = ToolbarControl::BrushSizeSlider.slider().unwrap().numeric().resolve(value.value, NumericOperation::Format).unwrap().fill;
            until(|| (scale.value() - expected).abs() < 0.00001, "native value and handle follow the shared stretched mapping");
            let label = d.named("slider-preview-label").downcast::<gtk::Label>().unwrap();
            let text = slider_preview_layout(ToolbarControl::BrushSizeSlider, TileStyle::Medium, state(&d.w).brush.diameter, 200., 1., &d.w.localization()).unwrap().text;
            assert_eq!(label.text(), text);
            if i == 2 { d.input.perform(serde_json::json!([{ "capture": "slider-stretch-far-preview" }])); }
        }
    }
    d.input.perform(serde_json::json!([contact("mouse", "up", points[points.len() - 1])]));
    capture(&mut d, "slider-stretch-return");
    d.input.perform(serde_json::json!([contact("mouse", "down", points[points.len() - 1]), contact("mouse", "up", points[points.len() - 1])]));
    let bookmark = d.named("slider-bookmark");
    let popup = bookmark.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>().unwrap();
    until(|| popup.is_mapped() && popup.can_target(), "the retained tap preview accepts input again");
    let count = state(&d.w).toolbar_component(ToolbarControl::BrushSizeSlider).unwrap().bookmarks.len();
    d.click(&bookmark);
    until(|| state(&d.w).toolbar_component(ToolbarControl::BrushSizeSlider).unwrap().bookmarks.len() != count, "the retained preview bookmark receives native input");
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_settings_small_bar --native-storage; 320x568 light"]
fn native_phone_settings_small_bar() {
    let mut d = setup("art.capycanvas.PhoneSettingsSmallBar");
    assert_eq!([d.w.surface.width(), d.w.surface.height()], [320, 568]);
    inside(&d, &d.named("phone-tool-settings"));
    for name in ["size", "strength"] {
        let scale = d.named(&format!("component-slider-{name}")).downcast::<gtk::Scale>().unwrap();
        let range = scale.range_rect();
        let (start, end) = scale.slider_range();
        let a = scale.compute_point(&d.w.window, &gtk::graphene::Point::new(range.x() as f32, 0.)).unwrap();
        let b = scale.compute_point(&d.w.window, &gtk::graphene::Point::new((range.x() + range.width() - end + start) as f32, 0.)).unwrap();
        let track = b.x() - a.x();
        assert!(track >= 70., "{name} retains a usable drag track: {track}");
        println!("320 {name} usable track {track:.2}");
    }
    capture(&mut d, "fixed-settings-small-bar");
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_color_resize --native-storage; 412x891 viewport, theme external"]
fn native_phone_color_resize() {
    let d = setup("art.capycanvas.PhoneColorResize");
    d.w.window.unmaximize();
    d.w.window.set_default_size(318, 568);
    until(|| !d.w.window.is_maximized() && [d.w.surface.width(), d.w.surface.height()] == [318, 568],
        "the unmaximized phone window is allocated before opening Color");
    for swapped in [false, true] {
        if state(&d.w).phone.swapped != swapped {
            d.w.dispatch(UiAction::Phone { action: PhoneAction::SwapSides });
            until(|| d.w.drawer.is_closed(), "Swap Sides closes the previous drawer");
        }
        let opener = tool(&d, ToolbarControl::Color);
        descendant::<gtk::Button>(&d.named(&opener)).unwrap().emit_clicked();
        until(|| d.w.drawer.placement().is_some() && !d.w.drawer.animation_active(), "Color opens before resizing");
        for size in [[317, 568], [318, 568], [280, 560], [320, 568], [318, 568]] {
            d.w.window.set_default_size(size[0], size[1]);
            until(|| [d.w.surface.width(), d.w.surface.height()] == size && !d.w.drawer.animation_active(),
                "the open Color drawer follows the resized window");
            color_drawer(&d);
        }
    }
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_color_budget --native-storage; viewport/theme external"]
fn native_phone_color_budget() {
    let mut d = setup("art.capycanvas.PhoneColorBudget");
    let opener = tool(&d, ToolbarControl::Color);
    click(&mut d, &opener);
    until(|| d.w.drawer.placement().is_some() && !d.w.drawer.animation_active()
        && find_named(d.w.window.upcast_ref(), "color-drawer-scroll")
            .is_some_and(|scroll| scroll.is_mapped() && scroll.width() > 0 && scroll.height() > 0),
        "the Color drawer is allocated");
    color_drawer(&d);
    capture(&mut d, "color-budget-top");
    let scroll = d.named("color-drawer-scroll").downcast::<gtk::ScrolledWindow>().unwrap();
    let adjustment = scroll.vadjustment();
    if adjustment.upper() > adjustment.page_size() + 0.5 {
        scroll_overflow(&mut d);
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
        until(|| (adjustment.value() + adjustment.page_size() - adjustment.upper()).abs() <= 0.5,
            "the whole Color drawer scrolls to its bottom");
        capture(&mut d, "color-budget-bottom");
    }
    d.input.key(0xff1b);
    d.finish();
}

fn paper_under_bars(d: &mut Driver) {
    let size = ui_session(&d.w).engine().document().composition().size;
    let zoom = 4. * (d.w.surface.width() as f32 / size[0] as f32).max(d.w.surface.height() as f32 / size[1] as f32);
    d.w.dispatch(UiAction::SetZoom { zoom });
    new_photo::ready(&d.w);
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_bar_surfaces --native-storage; phone viewport/theme external"]
fn native_phone_bar_surfaces() {
    let mut d = setup("art.capycanvas.PhoneBarSurfaces");
    frame(&d);
    capture(&mut d, "bar-no-grips");
    paper_under_bars(&mut d);
    d.input.key(0xff09);
    until(|| state(&d.w).phone.hidden, "Tab hides the bars over paper");
    capture_current(&mut d, "bar-hidden-over-paper");
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_bar_flicker --native-storage; 412x891 or891x412, theme external"]
fn native_phone_bar_flicker() {
    let mut d = setup("art.capycanvas.PhoneBarFlicker");
    let viewport = [d.w.surface.width(), d.w.surface.height()];
    assert!(matches!(viewport, [412, 891] | [891, 412]));
    paper_under_bars(&mut d);
    let theme = format!("{:?}", state(&d.w).theme).to_lowercase();
    for material in [0, 2] {
        d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit {
            id: PreferenceId::Transparency, value: PreferenceValue::Choice(material),
        } });
        for swapped in [false, true] {
            if state(&d.w).phone.hidden {
                d.input.key(0xff1b);
                until(|| !state(&d.w).phone.hidden, "Escape restores the next recording reference");
            }
            if state(&d.w).phone.swapped != swapped {
                d.w.dispatch(UiAction::Phone { action: PhoneAction::SwapSides });
            }
            let tag = format!("bar-flicker-{}x{}-{theme}-{material}-{swapped}", viewport[0], viewport[1]);
            clear_canvas_pointer(&mut d);
            let capy = d.named("zen-capy").compute_bounds(&d.w.surface).unwrap();
            let region = [capy.x().round() as i32, capy.y().round() as i32,
                capy.width().round() as i32, capy.height().round() as i32];
            frame(&d);
            d.input.key(0xff09);
            until(|| state(&d.w).phone.hidden, "prepare the rounded hidden reference");
            capture_current(&mut d, &format!("{tag}-hidden"));
            d.input.key(0xff1b);
            until(|| !state(&d.w).phone.hidden, "prepare the continuous bar reference");
            capture_current(&mut d, &format!("{tag}-rest"));
            println!("CAPY_FLICKER {}", serde_json::json!({
                "name": tag, "region": region, "viewport": viewport,
                "reference": format!("phone-{}x{}-{theme}-{tag}-rest.png", viewport[0], viewport[1]),
                "hidden_reference": format!("phone-{}x{}-{theme}-{tag}-hidden.png", viewport[0], viewport[1]),
            }));
            d.input.perform(serde_json::json!([{ "record_window": { "name": tag, "maximized": true } }]));
            for hidden in [true, false, true] {
                let w = d.w.clone();
                let frames = sample_after_paint(&d.w.window, move || state(&w).phone.hidden);
                d.input.key(if hidden { 0xff09 } else { 0xff1b });
                until(|| state(&d.w).phone.hidden == hidden, "native hide/show reaches its declared state");
                d.w.window.queue_draw();
                until(|| frames.samples.borrow().iter().any(|sample| sample.state == hidden),
                    "each recorded hide/show state reaches after-paint");
                println!("CAPY_STATES {}", serde_json::json!({ "name": tag, "frames": frames.finish().into_iter()
                    .map(|frame| serde_json::json!({ "time_us": frame.time, "hidden": frame.state })).collect::<Vec<_>>() }));
                let bounds = d.named("zen-capy").compute_bounds(&d.w.surface).unwrap();
                assert_eq!([bounds.x().round() as i32, bounds.y().round() as i32,
                    bounds.width().round() as i32, bounds.height().round() as i32], region,
                    "Capy keeps its exact native tile through hide/show");
                if hidden { assert_eq!(presentation(&d).chips[1], presentation(&d).capy); }
                d.input.perform(serde_json::json!([{ "wait_ms": 200 }]));
            }
            d.input.perform(serde_json::json!([{ "record_stop": true }]));
        }
    }
    d.finish();
}

fn open_menu_page(d: &mut Driver, label: &str) -> gtk::Stack {
    d.click_label(label);
    let stack = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    let name = menu_page_name(d, label);
    until(|| stack.is_mapped() && stack.visible_child_name().as_deref() == Some(name.as_str())
        && !stack.is_transition_running(), "the native menu page is ready");
    stack
}

fn menu_rows_released(stack: &gtk::Stack) {
    let active = widgets(stack.upcast_ref()).filter(|widget| widget.is_mapped()
        && widget.css_name() == "modelbutton" && widget.state_flags().contains(gtk::StateFlags::ACTIVE))
        .map(|widget| menu_text(&widget)).collect::<Vec<_>>();
    assert!(active.is_empty(), "a claimed scroll or swipe releases every native row: {active:?}");
}

fn menu_back_swipe(d: &mut Driver, stack: &gtk::Stack, initial: Bounds) {
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    let page = stack.visible_child().unwrap();
    assert!(gtk::prelude::GtkWindowExt::focus(&d.w.window).is_some_and(|focus| focus.is_ancestor(&page)),
        "a tapped menu row transfers keyboard focus into its subpage");
    let name = stack.visible_child_name().unwrap();
    let scroll = d.named("phone-menu-scroll").downcast::<gtk::ScrolledWindow>().unwrap();
    let sign = if scroll.direction() == gtk::TextDirection::Rtl { -1. } else { 1. };
    let mut from = screen_point(scroll.upcast_ref(), &d.w.window, [if sign < 0. { 0.85 } else { 0.15 }, 0.5]);
    if presentation(d).title.width > 0. {
        let bounds = scroll.compute_bounds(&d.w.surface).unwrap();
        assert!(bounds.y() <= 12. && bounds.y()+bounds.height() > 12., "submenu swipe content intersects the title band");
        let local = d.w.surface.compute_point(&d.w.window,&gtk::graphene::Point::new(bounds.x()+bounds.width()*0.15,12.)).unwrap();
        from = [local.x(),local.y()];
        let picked = d.w.window.pick(from[0] as f64,from[1] as f64,gtk::PickFlags::DEFAULT).unwrap();
        let menu = d.named("phone-menu");
        assert!(picked == menu || picked.is_ancestor(&menu),"the submenu owns the title overlap");
    }
    let width = stack.width() as f32;
    let point = |fraction: f32| [from[0] + sign * width * fraction, from[1]];
    d.input.perform(serde_json::json!([
        contact("touch", "down", from), contact("touch", "move", point(0.1)),
        contact("touch", "move", point(0.12)), contact("touch", "move", point(0.13)),
        { "wait_ms": 250 },
        contact("touch", "up", point(0.13))
    ]));
    until(|| stack.can_target(), "a short cancelled swipe restores list input");
    assert_eq!(stack.visible_child_name(), Some(name.clone()));
    assert!(d.w.visible_dialog().is_none());
    capture(d, "menu-swipe-cancelled");
    d.input.perform(serde_json::json!([
        contact("touch", "down", from), contact("touch", "move", point(0.2)),
        contact("touch", "move", point(0.5))
    ]));
    until(|| !stack.can_target(), "the back gesture owns the held touch");
    assert_eq!(stack.visible_child_name(), Some(name));
    assert!(stack.child_by_name("main").unwrap().is_child_visible(), "the live parent page is exposed behind the current page");
    menu_bounds(d, initial);
    menu_rows_released(stack);
    capture_current(d, "menu-swipe-halfway");
    d.input.perform(serde_json::json!([contact("touch", "up", point(0.5))]));
    until(|| stack.visible_child_name().as_deref() == Some("main") && stack.can_target()
        && !stack.is_transition_running(), "a completed native swipe returns to the parent");
    menu_bounds(d, initial);
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    capture(d, "menu-swipe-completed");
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_menu_gestures --native-storage; phone viewport/theme external"]
fn native_phone_menu_gestures() {
    let mut d = setup("art.capycanvas.PhoneMenuGestures");
    menu(&mut d);
    let initial = d.w.drawer.placement().as_ref().unwrap().bounds;
    let root = d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap();
    let longest = ui_session(&d.w).phone_menu().unwrap().view(&drawing_selector(&d), Platform::Gtk, &d.w.localization()).pages.into_iter().filter(|page| page.parent.as_ref().is_some_and(|parent| *parent == PhoneMenuRoute::default()))
        .max_by_key(|page| root.child_by_name(&page.id.name()).unwrap().measure(gtk::Orientation::Vertical, root.width()).1)
        .unwrap().title;
    let stack = open_menu_page(&mut d, &longest);
    let name = stack.visible_child_name().unwrap();
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    let drawing = d.w.documents.selected();
    let scroll = d.named("phone-menu-scroll").downcast::<gtk::ScrolledWindow>().unwrap();
    let adjustment = scroll.vadjustment();
    assert!(adjustment.upper() > adjustment.page_size() + 1., "the longest native submenu provides a real scrolling workload");
    let before_scroll = adjustment.value();
    let from = screen_point(scroll.upcast_ref(), &d.w.window, [0.5, 0.75]);
    let delta = (scroll.height() as f32 * 0.4).min(100.);
    d.input.perform(serde_json::json!([
        contact("touch", "down", from), contact("touch", "move", [from[0], from[1] - delta * 0.4]),
        contact("touch", "move", [from[0], from[1] - delta])
    ]));
    until(|| adjustment.value() > before_scroll + 1., "touch scrolling moves the native list from its initial offset");
    eprintln!("menu scroll adjustment: before={before_scroll}, after={}, upper={}, page={}", adjustment.value(), adjustment.upper(), adjustment.page_size());
    menu_rows_released(&stack);
    capture_current(&mut d, "menu-touch-scrolling");
    d.input.perform(serde_json::json!([contact("touch", "up", [from[0], from[1] - delta])]));
    assert_eq!(stack.visible_child_name(), Some(name.clone()));
    assert!(d.w.visible_dialog().is_none(), "scrolling never activates a command");
    assert_eq!(d.w.documents.selected(), drawing);
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    capture(&mut d, "menu-touch-scrolled");
    inventory_close(&mut d);
    menu(&mut d);
    let file = ApplicationMenu::File.localized_label(&d.w.localization());
    let stack = open_menu_page(&mut d, &file);
    menu_back_swipe(&mut d, &stack, initial);
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    d.finish();
}

fn inventory_close(d: &mut Driver) {
    if let Some(dialog) = d.w.visible_dialog() { crate::page_dialog::close_dialog(&dialog); }
    if state(&d.w).phone.menu { d.w.dispatch(UiAction::Phone { action: PhoneAction::Capy }); }
    if state(&d.w).customization.drawer.is_some() {
        d.w.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded });
    }
    if ui_session(&d.w).command(CommandId::CancelTransform).enabled {
        d.w.dispatch(UiAction::Invoke { command: CommandId::CancelTransform });
    }
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running()
        && !state(&d.w).phone.menu && state(&d.w).customization.drawer.is_none()
        && d.w.drawer.is_closed(), "the inventory surface closes before the next view");
}

fn inventory_drawer_ready(d: &Driver) {
    until(|| state(&d.w).customization.drawer.is_some() && d.w.drawer.placement().is_some()
        && !d.w.drawer.animation_active(), "the inventory drawer finishes opening");
    until(|| {
        let view = state(&d.w);
        let drawer = view.customization.drawer.as_ref().unwrap();
        let panels = drawer.columns.iter().flatten().copied().collect::<Vec<_>>();
        if panels.contains(&Panel::Layers) && drawer.layer_properties != Some(true) {
            let Some(layers) = d.w.drawer.layers() else { return false; };
            let Some(id) = view.layer_tools.editing_layer.as_ref().map(|row| row.id) else { return false; };
            if layers.list.width() <= 0 || layers.list.height() <= 0 || !find_named(layers.list.upcast_ref(), &format!("art-layer-{id}"))
                .filter(|row| row.is_mapped()).and_then(|row| row.compute_bounds(&layers.list))
                .is_some_and(|bounds| bounds.width() > 0. && bounds.height() > 0.
                    && bounds.y() >= -1. && bounds.y() + bounds.height() <= layers.list.height() as f32 + 1.) { return false; }
        }
        if panels.contains(&Panel::Color) {
            let Some(content) = find_named(d.w.window.upcast_ref(), "drawer-panel-Color").filter(|widget| widget.is_mapped()) else { return false; };
            let Some(wheel) = find_named(&content, "color-wheel").and_downcast::<crate::tool_panels::ColorWheel>() else { return false; };
            let Some(scroll) = wheel.ancestor(gtk::ScrolledWindow::static_type()) else { return false; };
            let (diameter, origin) = wheel.drawing_bounds();
            if !wheel.is_mapped() || scroll.width() <= 0 || scroll.height() <= 0 || wheel.width() <= 0
                || diameter < 127.5 || origin[1] < -0.5 || origin[1] + diameter > wheel.height() as f32 + 0.5 { return false; }
        }
        true
    }, "the inventory drawer has allocated its visible contents");
}

fn inventory_monitor(d: &mut Driver, name: &str) {
    let frames = sample_after_paint(&d.w.window, || ());
    d.w.window.queue_draw();
    until(|| !frames.samples.borrow().is_empty(), "the inventory overlay has been painted");
    drop(frames.finish());
    let theme = format!("{:?}", state(&d.w).theme).to_lowercase();
    let capture_name = format!("inventory-{}x{}-{theme}-{name}", d.w.surface.width(), d.w.surface.height());
    d.input.perform(serde_json::json!([{ "capture": &capture_name }]));
    super::phone_motion::export_allocations(&d.w,name,&capture_name);
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_inventory --native-storage; phone viewport/theme external"]
fn native_phone_inventory() {
    let mut d = setup("art.capycanvas.PhoneInventory");
    let sample = layer_core::color::source::rgba8_source([64, 64], |x, y|
        if (x / 16 + y / 16) % 2 == 0 { [76, 112, 144, 255] } else { [208, 176, 128, 255] });
    ui_session_mut(&d.w).import_layer_source("Inventory marks", std::sync::Arc::unwrap_or_clone(sample)).unwrap();
    d.w.wake();
    new_photo::ready(&d.w);
    let compact = d.w.surface.width().min(d.w.surface.height()) < 320;
    for preset in WorkspacePreset::ALL {
        let tag = preset.name().to_lowercase();
        d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
            layout: preset.layout(Platform::Gtk), ..Default::default()
        }) });
        until(|| state(&d.w).workspace.layout.phone.as_ref().is_some_and(|tiles|
            tiles.iter().map(|tile| tile.control).collect::<Vec<_>>() == preset.phone_controls()),
            "the inventory workspace uses its actual phone preset");
        capture(&mut d, &format!("inventory-{tag}-rest"));
        if preset == WorkspacePreset::Painter {
            d.w.dispatch(UiAction::Phone { action: PhoneAction::SwapSides });
            capture(&mut d, "inventory-swapped");
            d.w.dispatch(UiAction::Phone { action: PhoneAction::SwapSides });
            let w = d.w.clone();
            let frames = sample_after_paint(&d.w.window, move || state(&w).phone.hidden
                && std::iter::successors(w.phone.root.first_child(), |child| child.next_sibling())
                    .all(|child| child.is_mapped() == (child.widget_name() == "phone-sliders-chip"
                        || (child.widget_name() == "phone-title-band" && state(&w).phone_presentation(
                            [w.surface.width() as f32,w.surface.height() as f32]).unwrap().title.width > 0.)))
                && find_named(w.phone.root.upcast_ref(), "component-slider-size").is_some_and(|slider| !slider.is_mapped()));
            d.input.key(0xff09);
            until(|| frames.samples.borrow().iter().any(|sample| sample.state), "Zen's hidden controls reach native after-paint");
            drop(frames.finish());
            capture(&mut d, "inventory-hidden");
            d.input.key(0xff1b);
            until(|| !state(&d.w).phone.hidden, "Escape restores the inventory bars");
        }
        if compact && preset != WorkspacePreset::Painter { continue; }
        let tiles = state(&d.w).workspace.layout.phone.unwrap();
        for tile in tiles.into_iter().filter(|tile| tile.control != ToolbarControl::Divider) {
            if compact && !matches!(tile.control, ToolbarControl::Color
                | ToolbarControl::Panel { panel: Panel::Layers }
                | ToolbarControl::Command { command: CommandId::DrawingBrush }) { continue; }
            let name = format!("phone-tool-{}", tile.id);
            click(&mut d, &name);
            new_photo::ready(&d.w);
            if state(&d.w).customization.drawer.is_none() { click(&mut d, &name); }
            inventory_drawer_ready(&d);
            let label = match tile.control {
                ToolbarControl::Command { command } => format!("{command:?}"),
                ToolbarControl::ToolSlot { slot } => format!("{slot:?}"),
                ToolbarControl::Panel { panel } => format!("{panel:?}"),
                _ => "Color".to_string(),
            }.to_lowercase();
            capture(&mut d, &format!("inventory-{tag}-{label}"));
            inventory_close(&mut d);
        }
    }
    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
        layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..Default::default()
    }) });
    for command in [CommandId::DrawingBrush, CommandId::Fill, CommandId::Select] {
        if compact && command != CommandId::DrawingBrush { continue; }
        d.w.dispatch(UiAction::Invoke { command });
        click(&mut d, "phone-tool-settings");
        inventory_drawer_ready(&d);
        capture(&mut d, &format!("inventory-settings-{}", format!("{command:?}").to_lowercase()));
        inventory_close(&mut d);
    }
    d.w.documents.enqueue(&d.w, (new_drawing_at(128, 96, layer_core::color::SampleDepth::U8), None));
    until(|| d.w.documents.len() == 2 && !d.w.documents.changing.get(), "two drawings are ready for the inventory");
    new_photo::ready(&d.w);
    menu(&mut d);
    capture(&mut d, "inventory-menu-root");
    click(&mut d, "phone-drawing");
    until(|| drawings_ready(&d), "the inventory Drawings page is ready");
    capture(&mut d, "inventory-menu-drawings");
    click(&mut d, "phone-menu-back");
    until(|| d.named("phone-menu-pages").downcast::<gtk::Stack>().unwrap().visible_child_name().as_deref() == Some("main"), "Drawings returns to the inventory root");
    if d.w.surface.height() > 280 {
        let initial = d.w.drawer.placement().as_ref().unwrap().bounds;
        let label = ApplicationMenu::File.localized_label(&d.w.localization());
        let stack = open_menu_page(&mut d, &label);
        capture(&mut d, "inventory-menu-file");
        menu_back_swipe(&mut d, &stack, initial);
    }
    if !compact {
        let label = ApplicationMenu::Window.localized_label(&d.w.localization());
        open_menu_page(&mut d, &label);
        open_menu_page(&mut d, "Workspaces");
        capture(&mut d, "inventory-menu-workspaces");
        inventory_close(&mut d);
        menu(&mut d);
        menu_layers(&mut d);
        inventory_drawer_ready(&d);
        capture(&mut d, "inventory-capy-layers");
        inventory_close(&mut d);
        d.w.dispatch(UiAction::Invoke { command: CommandId::DrawingBrush });
        let scale = d.named("component-slider-size").downcast::<gtk::Scale>().unwrap();
        let from = d.point(scale.upcast_ref());
        let phone = presentation(&d);
        let along = if phone.edge == Edge::Top { [from[0] + 12., from[1]] } else { [from[0], from[1] - 12.] };
        d.input.perform(serde_json::json!([contact("mouse", "down", from), contact("mouse", "move", along)]));
        until(|| d.named("brush-slider-preview").is_mapped(), "the track preview is visible");
        inventory_monitor(&mut d, "slider-track");
        let phone = presentation(&d);
        let far = if phone.edge == Edge::Top { [from[0], from[1] - phone.slider_stretch_distance()] }
            else { [from[0] + phone.slider_stretch_distance(), from[1]] };
        d.input.perform(serde_json::json!([contact("mouse", "move", far)]));
        inventory_monitor(&mut d, "slider-away");
        d.input.perform(serde_json::json!([contact("mouse", "up", far)]));
        d.input.key(0xff1b);
        d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit {
            id: PreferenceId::FingerDrawing, value: PreferenceValue::Choice(1),
        } });
        d.w.dispatch(UiAction::SetColor { rgba: [0., 0., 0., 1.] });
        until(|| {
            let gpu = d.w.gpu.borrow();
            let engine = gpu.as_ref().unwrap().session.engine();
            engine.backend().paint_ready(engine.document(), engine.brush(), false)
        }, "the inventory brush is ready before holding");
        let point = screen_point(d.w.surface.upcast_ref(), &d.w.window, [0.5, 0.5]);
        let hold = gtk::Settings::default().unwrap().gtk_long_press_time() + 100;
        d.input.perform(serde_json::json!([{ "touch": "down", "point": point }, { "wait_ms": hold }]));
        until(|| ui_session(&d.w).color_picker_overlay().is_some(), "the inventory finger hold exposes its loupe");
        inventory_monitor(&mut d, "finger-hold");
        d.input.perform(serde_json::json!([{ "touch": "up" }]));
        until(|| ui_session(&d.w).color_picker_overlay().is_none(), "the inventory hold finishes");
    } else { inventory_close(&mut d); }
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_transform_escape --native-storage; 412x891 or1600x1000 light/dark"]
fn native_transform_escape() {
    let mut d = setup("art.capycanvas.TransformEscape");
    let sample = layer_core::color::source::rgba8_source([64, 64], |x, y|
        if (x / 16 + y / 16) % 2 == 0 { [76, 112, 144, 255] } else { [208, 176, 128, 255] });
    ui_session_mut(&d.w).import_layer_source("Escape marks", std::sync::Arc::unwrap_or_clone(sample)).unwrap();
    d.w.wake();
    new_photo::ready(&d.w);
    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
        layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..Default::default()
    }) });
    let control = ToolbarControl::Command { command: CommandId::ScaleRotate };
    let phone = d.w.surface.width().min(d.w.surface.height()) < 480;
    let opener = if phone { tool(&d, control) } else {
        let layout = state(&d.w).workspace.layout;
        layout.header.entries().filter(|entry| entry.item == HeaderItem::Tool { control })
            .map(|entry| format!("header-item-{}", entry.id))
            .chain(layout.panels.iter().flat_map(|panel| panel.tiles()).filter(|tile| tile.control == control)
                .map(|tile| format!("tile-{}", tile.id)))
            .find(|name| find_named(d.w.window.upcast_ref(), name).is_some_and(|widget| widget.is_mapped())).unwrap()
    };
    click(&mut d, &opener);
    new_photo::ready(&d.w);
    if phone && state(&d.w).customization.drawer.is_none() { click(&mut d, &opener); }
    until(|| ui_session(&d.w).command(CommandId::CancelTransform).enabled,
        "Scale/rotate has a cancellable transform");
    d.w.dispatch(UiAction::SetToolSetting { id: "transform_angle".into(), value: 0.3 });
    new_photo::ready(&d.w);
    assert!((state(&d.w).tool_settings.iter().find(|setting| setting.id == "transform_angle").unwrap().value - 0.3).abs() < 1e-6);
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    capture_current(&mut d, "transform-escape-before");
    let frames = sample_after_paint(&d.w.window, || ());
    d.input.key(0xff1b);
    d.w.window.queue_draw();
    until(|| !frames.samples.borrow().is_empty() && !d.w.drawer.animation_active(), "physical Escape reaches native after-paint");
    drop(frames.finish());
    capture_current(&mut d, "transform-escape-after");
    assert!(state(&d.w).customization.drawer.is_none() && d.w.drawer.is_closed(),
        "one physical Escape closes the transform drawer");
    assert!(!ui_session(&d.w).command(CommandId::CancelTransform).enabled,
        "one physical Escape cancels the pending transform");
    assert!(state(&d.w).canvas_bar.is_none(), "the cancelled transform leaves no canvas bar");
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint, "Escape never commits the pending transform");
    d.finish();
}

#[path = "phone_customize_tests.rs"]
mod customize;
