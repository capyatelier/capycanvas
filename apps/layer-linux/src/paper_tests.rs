use super::new_photo::{chooser, ready, response};
use super::*;
use layer_render::CanvasRenderer;
use layer_core::{DefaultBrushPreset, EffectValue};

fn shade(w: &Rc<Workspace>, origin: [f32; 2], pressure: f32) {
    let deadline = Instant::now() + Duration::from_secs(30);
    until(|| Instant::now() > deadline || w.gpu.borrow().as_ref().is_some_and(|g| {
        let engine = g.session.engine();
        engine.backend().paint_ready(engine.document(), engine.brush(), engine.transform_preview().is_some())
    }), "brush and paper are ready");
    let camera = state(w).camera;
    let points: Vec<[f32; 2]> = (0..=180).map(|i| {
        let t = i as f32 / 180.;
        [origin[0] + 220. * ((t * 18.).fract() - 0.5).abs() * 2., origin[1] + 140. * t]
    }).collect();
    for (i, point) in points.iter().enumerate() {
        let phase = match i { 0 => PenPhase::Down, _ if i + 1 == points.len() => PenPhase::Up, _ => PenPhase::Move };
        let now = glib::monotonic_time() as u64 * 1000;
        let event = pen_event(&camera, *point, phase, now);
        w.input.send(w, PenEvent { device_id: 93, timestamp_ns: now, pressure, ..event });
        pump(8);
    }
    pump(200);
}

fn texture(w: &Rc<Workspace>) -> layer_core::TextureSource {
    ui_session(w).engine().document().paper(None).texture
}

fn texture_dialog(w: &Rc<Workspace>, opener: &gtk::Button) -> (adw::Dialog, gtk::FlowBox) {
    click(opener);
    until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "texture-dialog" && dialog.is_mapped()), "Choose Texture opens");
    pump(300);
    let dialog = w.window.visible_dialog().unwrap();
    let grid: gtk::FlowBox = named(dialog.upcast_ref(), "texture-grid");
    (dialog, grid)
}

fn texture_button(dialog: &adw::Dialog, name: &str) -> gtk::Button {
    named(dialog.upcast_ref(), &format!("texture-{name}"))
}

fn content_size(dialog: &adw::Dialog) -> (i32, i32) {
    let content = dialog.child().unwrap();
    (content.width(), content.height())
}

fn built_in() -> i32 {
    layer_core::paper::built_in_papers().len() as i32
}

fn drawing() -> Option<i32> {
    Some(index(layer_core::paper::DEFAULT_PAPER))
}

fn index(id: &str) -> i32 {
    layer_core::paper::built_in_papers().iter().position(|paper| paper.id.as_ref() == id).unwrap() as i32
}

fn pending(grid: &gtk::FlowBox) -> Option<i32> {
    grid.selected_children().first().map(|child| child.index())
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_paper_picker_relief_and_scale() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.PaperPicker");
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let project = layer_ui::NewDocumentOptions { extent: [1600, 1000], ..Default::default() }.project(&localization).unwrap();
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.maximize();
    w.window.present();
    ready(&w);
    let theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "default".into());
    let directory = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| "../../artifacts/paper".into()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = |name: &str| directory.join(format!("{name}-{theme}.png"));

    w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Pencil as u32 });
    shade(&w, [180., 200.], 0.45);
    w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Charcoal as u32 });
    shade(&w, [480., 200.], 0.45);

    let paper = layer_ui::occurrence_token(layer_core::paper::paper_occurrence(ui_session(&w).engine().document().scene(), None).unwrap());
    w.dispatch(UiAction::SelectLayer { id: paper });
    ready(&w);
    let properties = state(&w).layer_properties;
    assert_eq!(properties.name, "Paper");
    assert_eq!(properties.controls.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["color", "texture", "scale", "relief", "light", "through_paint"]);
    save_snapshot(&w, 200, || path("paper-properties"));

    let button: gtk::Button = named(w.window.upcast_ref(), "property-texture");
    assert!(button.is_mapped(), "the Texture tile is visible in Properties");
    let (dialog, grid) = texture_dialog(&w, &button);
    save_snapshot(&w, 200, || path("texture-dialog"));
    assert_eq!(dialog.title(), "Choose Texture");
    assert_eq!(grid.observe_children().n_items(), built_in() as u32);
    assert_eq!(pending(&grid), drawing(), "Fine tooth is selected");
    let size = content_size(&dialog);
    assert_eq!(size, (720, 560), "Choose Texture is Edit Color's width and shows three full rows");
    let tile = grid.child_at_index(0).unwrap();
    let preview = named::<gtk::Image>(tile.upcast_ref(), "texture-preview");
    assert_eq!((preview.width(), preview.height()), (96, 96), "previews show at their own size");
    let footer = |name: &str| texture_button(&dialog, name).compute_bounds(&dialog).unwrap();
    assert!(footer("import").x() < footer("cancel").x() && footer("cancel").x() < footer("apply").x(), "Import, Cancel, Use Texture");
    assert_eq!(texture_button(&dialog, "import").label().as_deref(), Some("Import…"));
    assert_eq!(texture_button(&dialog, "apply").label().as_deref(), Some("Use Texture"));
    let before = ui_session(&w).engine().checkpoint();
    grid.select_child(&grid.child_at_index(2).unwrap());
    click(&texture_button(&dialog, "cancel"));
    until(|| !dialog.is_mapped(), "Cancel closes Choose Texture");
    assert_eq!(ui_session(&w).engine().checkpoint(), before, "Cancel changes nothing");
    let (dialog, grid) = texture_dialog(&w, &button);
    assert_eq!(content_size(&dialog), size);
    assert_eq!(pending(&grid), drawing(), "a cancelled choice is not kept");
    grid.select_child(&grid.child_at_index(2).unwrap());
    save_snapshot(&w, 100, || path("texture-dialog-chosen"));
    assert_eq!(texture(&w), layer_core::paper::default_texture(), "nothing changes before Use Texture");
    click(&texture_button(&dialog, "apply"));
    until(|| !dialog.is_mapped(), "Use Texture closes Choose Texture");
    assert_eq!(texture(&w), layer_core::paper::built_in_paper("rough").unwrap().texture);
    pump(250);
    save_snapshot(&w, 50, || path("paper-relief-rough"));
    assert!(ui_session(&w).engine().backend().has_paper_overlay(), "the relief shows after a choice");
    pump(1800);
    assert!(!ui_session(&w).engine().backend().has_paper_overlay(), "and then hides");

    let scale: crate::number_control::NumberControl = named(w.window.upcast_ref(), "property-scale");
    edit_number(&scale, "200");
    pump(200);
    assert_eq!(state(&w).layer_properties.controls[2].value, EffectValue::Number(2.));
    save_snapshot(&w, 50, || path("paper-relief-scaled"));
    pump(1800);

    let scan = directory.join("cotton-rag.png");
    let pixels: Vec<u8> = (0..400u32 * 300).flat_map(|i| {
        let (x, y) = (i % 400, i / 400);
        let v = (128. + 60. * ((x as f32 * 0.37).sin() * (y as f32 * 0.29).cos()) + ((x * 7919 + y * 104_729) % 61) as f32 - 30.) as u8;
        [v, v, v, 255]
    }).collect();
    gtk::gdk::MemoryTexture::new(400, 300, gtk::gdk::MemoryFormat::R8g8b8a8, &glib::Bytes::from_owned(pixels), 1600).save_to_png(&scan).unwrap();
    let (dialog, _) = texture_dialog(&w, &button);
    click(&texture_button(&dialog, "import"));
    until(|| !dialog.is_mapped(), "Import closes Choose Texture");
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(&scan)).unwrap();
    pump(150);
    file.response(gtk::ResponseType::Accept);
    until(|| matches!(texture(&w), layer_core::TextureSource::Image(_)), "the imported paper is applied");
    let tiles = || state(&w).layer_properties.controls[1].paper.clone().unwrap().tiles;
    assert_eq!(tiles().len(), built_in() as usize + 1);
    assert_eq!(tiles()[built_in() as usize].label.as_ref(), "cotton-rag");
    assert_eq!(state(&w).layer_properties.controls[1].value, EffectValue::Choice(built_in() as u32));
    let imported = ui_session(&w).engine().checkpoint();
    let remembered = ui_session(&w).state().settings.new_document.paper.clone().unwrap();
    assert_eq!(remembered.len(), 64, "new drawings start on the imported paper");
    pump(1800);
    let button: gtk::Button = named(w.window.upcast_ref(), "property-texture");
    let (textures, grid) = texture_dialog(&w, &button);
    assert_eq!(pending(&grid), Some(built_in()), "the imported paper is chosen");
    let scroller = grid.ancestor(gtk::ScrolledWindow::static_type()).unwrap();
    let chosen = grid.child_at_index(built_in()).unwrap().compute_bounds(&scroller).unwrap();
    assert!(chosen.y() >= 0. && chosen.y() + chosen.height() <= scroller.height() as f32, "the chosen paper is scrolled into view: {chosen:?}");
    save_snapshot(&w, 200, || path("texture-dialog-imported"));
    let caption = |index: i32| grid.child_at_index(index).and_then(|child| child.tooltip_text()).unwrap_or_default();
    let menu = |index: i32, label: &str| {
        let child = grid.child_at_index(index).unwrap();
        let press = child.observe_controllers().iter::<glib::Object>().filter_map(Result::ok)
            .find_map(|controller| controller.downcast::<gtk::GestureClick>().ok()).expect("the tile has a menu");
        press.emit_by_name::<()>("pressed", &[&1i32, &0f64, &0f64]);
        pump(200);
        let menu: gtk::Popover = named(child.upcast_ref(), "paper-tile-menu");
        assert!(menu.is_visible());
        click(&find_button(menu.upcast_ref(), label).unwrap());
        pump(200);
    };
    menu(built_in(), "Rename…");
    let dialog = w.window.visible_dialog().unwrap();
    named::<adw::EntryRow>(dialog.upcast_ref(), "paper-rename-name").set_text("Cotton rag");
    pump(100);
    response(&w, "rename");
    until(|| tiles()[built_in() as usize].label.as_ref() == "Cotton rag" && caption(built_in()) == "Cotton rag", "the paper is renamed in the open dialog");
    assert!(textures.is_mapped() && pending(&grid) == Some(built_in()));
    menu(built_in(), "Delete");
    until(|| tiles().len() == built_in() as usize + 1 && tiles()[0].label.as_ref() == "This drawing’s paper" && caption(0) == "This drawing’s paper", "the drawing keeps its deleted paper");
    assert_eq!(pending(&grid), Some(0), "the choice follows the drawing's paper");
    assert!(matches!(texture(&w), layer_core::TextureSource::Image(_)));
    save_snapshot(&w, 200, || path("texture-dialog-own"));
    menu(0, "Add to Library");
    until(|| tiles().len() == built_in() as usize + 1 && tiles()[built_in() as usize].label.as_ref() == state(&w).document_file.title(), "the drawing's paper returns to the library");
    assert_eq!(state(&w).layer_properties.controls[1].value, EffectValue::Choice(built_in() as u32));
    until(|| pending(&grid) == Some(built_in()), "the open dialog follows the library");
    click(&texture_button(&textures, "cancel"));
    until(|| !textures.is_mapped(), "Choose Texture closes");

    let ink = layer_ui::occurrence_token(ui_session(&w).engine().document().scene().order()[0]);
    w.dispatch(UiAction::SelectLayer { id: ink });
    w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Pencil as u32 });
    shade(&w, [180., 520.], 0.45);
    w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Charcoal as u32 });
    shade(&w, [480., 520.], 0.45);
    ready(&w);
    save_snapshot(&w, 200, || path("paper-rough-strokes"));

    for _ in 0..2 { w.dispatch(UiAction::Invoke { command: CommandId::Undo }); }
    ready(&w);
    assert_eq!(ui_session(&w).engine().checkpoint(), imported, "each stroke is one step; library changes are none");
    for _ in 0..3 { w.dispatch(UiAction::Invoke { command: CommandId::Undo }); }
    ready(&w);
    assert_eq!(ui_session(&w).engine().checkpoint(), before, "the import, Scale and the choice are one step each");
    assert_eq!(texture(&w), layer_core::paper::default_texture());

    let row = || state(&w).layers.into_iter().find(|row| row.id == paper).unwrap();
    assert_eq!(row().paper.unwrap().label.as_ref(), "Fine tooth");
    let thumbnails = || {
        let row: gtk::Box = named(w.window.upcast_ref(), &format!("art-layer-{paper}"));
        let buttons: Vec<gtk::Button> = descendants::<gtk::Button>(row.upcast_ref()).into_iter().filter(|b| b.has_css_class("layer-thumbnail") && b.is_visible()).collect();
        let name = descendants::<gtk::Label>(row.upcast_ref()).into_iter().find(|label| label.has_css_class("layer-name")).unwrap();
        let x = |widget: &gtk::Widget| widget.compute_bounds(&row).unwrap();
        assert!(buttons.windows(2).all(|pair| x(pair[0].upcast_ref()).x() + x(pair[0].upcast_ref()).width() <= x(pair[1].upcast_ref()).x()));
        let last = x(buttons.last().unwrap().upcast_ref());
        assert!(last.x() + last.width() <= x(name.upcast_ref()).x(), "the texture sits left of the name");
        buttons
    };
    w.dispatch(UiAction::SelectLayer { id: ink });
    ready(&w);
    let buttons = thumbnails();
    assert_eq!(buttons.len(), 2, "Paper shows its color and its texture");
    assert_eq!(buttons[1].widget_name(), format!("layer-texture-{paper}"));
    assert_eq!(buttons[1].tooltip_text().as_deref(), Some("Choose Texture"));
    save_snapshot(&w, 200, || path("paper-row-texture"));
    let (dialog, grid) = texture_dialog(&w, &buttons[1]);
    assert_eq!(state(&w).layer_properties.layer, Some(paper), "the texture thumbnail selects its Paper");
    assert_eq!(pending(&grid), drawing());
    grid.select_child(&grid.child_at_index(index("bristol")).unwrap());
    click(&texture_button(&dialog, "apply"));
    until(|| !dialog.is_mapped(), "Use Texture closes Choose Texture");
    assert_eq!(texture(&w), layer_core::paper::built_in_paper("bristol").unwrap().texture);
    until(|| row().paper.unwrap().id.as_deref() == Some("bristol"), "the row shows the chosen texture");
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Lock { id: paper, value: true } });
    ready(&w);
    assert!(!row().paper.unwrap().editable);
    click(&thumbnails()[1]);
    pump(300);
    assert!(w.window.visible_dialog().is_none(), "a locked Paper keeps its texture");
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_fill_drawing_targets_and_paper_order() {
    let app = native_test_app("art.capycanvas.FillTargets");
    let w = fixture_workspace(&app);
    apply_fixture_theme(&w);
    w.window.maximize();
    w.window.present();
    ready(&w);
    let mut input = RemoteInput::new().settle_ms(120);
    input.ready();
    let directory = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| "../../artifacts/paper-bottom".into()));
    std::fs::create_dir_all(&directory).unwrap();
    for program in ["paper", "solid_color", "gradient_fill"] {
        w.dispatch(UiAction::Effect { action: layer_ui::EffectAction::Insert { effect: program.into() } });
        ready(&w);
        let fill = layer_ui::occurrence_token(ui_session(&w).engine().document().working.occurrence.unwrap());
        w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::New { group: false, clipped: false } });
        ready(&w);
        let ink = layer_ui::occurrence_token(ui_session(&w).engine().document().working.occurrence.unwrap());
        let change = ui_session_mut(&w).reveal_panel(Panel::Layers);
        w.changed(change);
        ready(&w);
        let row: gtk::Widget = named(w.window.upcast_ref(), &format!("art-layer-{fill}"));
        assert!(row.is_mapped());
        let bounds = find_css(&row, "layer-name").unwrap().compute_bounds(&w.surface).unwrap();
        let point = [bounds.x() + bounds.width() * 0.5, bounds.y() + bounds.height() * 0.5];
        input.perform(serde_json::json!([{"point":point}, {"down":true}, {"down":false}]));
        ready(&w);
        let fill_row = || state(&w).layers.into_iter().find(|row| row.id == fill).unwrap();
        let ink_row = || state(&w).layers.into_iter().find(|row| row.id == ink).unwrap();
        save_snapshot(&w, 100, || directory.join(format!("{program}-selected.png")));
        assert!(fill_row().editing && ink_row().drawing, "{program}: selecting a fill paints above it: fill={:?}, ink={:?}", fill_row(), ink_row());
        if program == "paper" {
            assert_eq!(state(&w).layers.last().unwrap().id, fill);
            assert!(!row.last_child().unwrap().is_visible());
        }
        let before = ink_row().paint_revision;
        shade(&w, [180., 200.], 0.45);
        ready(&w);
        assert_ne!(ink_row().paint_revision, before, "{program}: the layer above receives the stroke");
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        ready(&w);
        w.dispatch(UiAction::SelectLayer { id: ink });
        w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::GroupSelected });
        w.dispatch(UiAction::SelectLayer { id: fill });
        ready(&w);
        assert!(state(&w).layers.iter().all(|row| !row.drawing), "{program}: a grouped layer is not a fallback");
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        if program == "paper" {
            w.dispatch(UiAction::SelectLayer { id: fill });
            ready(&w);
            let add_mask = widgets(w.layer_panel.footer.upcast_ref()).filter_map(|node| node.downcast::<gtk::Button>().ok())
                .find(|button| button.tooltip_text().as_deref() == Some("Add mask")).unwrap();
            assert!(!add_mask.is_sensitive(), "Paper has no Add mask button");
            input.perform(serde_json::json!([{"point":point}, {"button":273, "down":true}, {"button":273, "down":false}]));
            let mask = super::canvas_bar_tests::until_some(|| find_menu_item(w.layer_panel.root.upcast_ref(), "Mask").filter(|item| item.is_mapped()), "the Paper row menu");
            let submenu = mask.property::<Option<gtk::PopoverMenu>>("popover").unwrap();
            let point = screen_point(&mask, &w.window, [0.5, 0.5]);
            input.perform(serde_json::json!([{"point":point}, {"wait_ms":200}]));
            if !submenu.is_mapped() { input.click(point); }
            until(|| submenu.is_mapped(), "the Paper Mask submenu opens");
            for label in ["Add mask", "Mask: reveal selection", "Mask: hide selection", "Paste mask"] {
                let item = super::canvas_bar_tests::until_some(|| find_menu_item(w.layer_panel.root.upcast_ref(), label).filter(|item| item.is_mapped()), label);
                assert!(!item.is_sensitive(), "Paper offers no {label}");
            }
            capture_popover(submenu.upcast_ref(), directory.join("paper-mask-menu.png").to_str().unwrap());
            let menu = mask.ancestor(gtk::Popover::static_type()).unwrap();
            for popover in [submenu.upcast_ref::<gtk::Widget>(), &menu] {
                if popover.is_visible() { input.key(0xff1b); }
                until(|| !popover.is_visible(), "Escape closes the Paper menus");
            }
            ready(&w);
            assert!(fill_row().mask_id.is_none());
            w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Delete { id: ink } });
            ready(&w);
            continue;
        }
        w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::AddMask { id: fill, replace: false } });
        w.dispatch(UiAction::SelectLayer { id: fill });
        ready(&w);
        assert!(fill_row().mask_selected && !ink_row().drawing, "{program}: selecting a masked fill targets its mask");
        let before = fill_row().mask_revision;
        shade(&w, [180., 200.], 0.45);
        ready(&w);
        assert_ne!(fill_row().mask_revision, before, "{program}: the mask receives the stroke");
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::DeleteMask { id: fill } });
        w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Delete { id: ink } });
        w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Delete { id: fill } });
        ready(&w);
    }
    let paper = layer_ui::occurrence_token(ui_session(&w).engine().document().bottom_paper().unwrap());
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Delete { id: paper } });
    ready(&w);
    let add = || widgets(w.window.upcast_ref()).filter_map(|node| node.downcast::<gtk::Box>().ok())
        .find(|row| row.widget_name() == "layer-add-paper" && row.is_mapped());
    let row = add().unwrap();
    let surface = row.parent().unwrap();
    assert!(descendant::<gtk::Button>(&row).is_none(), "the list row owns all action feedback");
    let label: gtk::Label = descendant(&row).unwrap();
    assert_eq!(label.layout().iter().run_readonly().unwrap().item().analysis().font().describe().weight(), gtk::pango::Weight::Normal, "Add paper uses regular text like the layer names");
    let bounds = row.compute_bounds(&w.surface).unwrap();
    let label_bounds = label.compute_bounds(&w.surface).unwrap();
    let (_, text_y) = label.layout_offsets();
    let (_, text_bounds) = label.layout().pixel_extents();
    let text_center = label_bounds.y() + text_y as f32 + text_bounds.y() as f32 + text_bounds.height() as f32 * 0.5;
    assert!((text_center - bounds.y() - bounds.height() * 0.5).abs() <= 1., "the text is vertically centered");
    assert!(bounds.height() - text_bounds.height() as f32 >= (crate::panel_controls::CONTROL_SPACING * 2) as f32, "the text has room above and below");
    let check_position = || {
        let row = add().unwrap();
        let last = state(&w).layers.last().unwrap().id;
        let last: gtk::Widget = named(w.window.upcast_ref(), &format!("art-layer-{last}"));
        let layer_bounds = last.compute_bounds(&w.surface).unwrap();
        let bounds = row.compute_bounds(&w.surface).unwrap();
        assert_eq!(bounds.height(), layer_bounds.height(), "Add paper has the standard layer row height");
        assert!((bounds.y() - layer_bounds.y() - layer_bounds.height()).abs() <= 2., "Add paper follows the last layer: {layer_bounds:?} {bounds:?}");
        let content = row.first_child().unwrap().compute_bounds(&w.surface).unwrap();
        assert!((content.x() + content.width() * 0.5 - bounds.x() - bounds.width() * 0.5).abs() <= 1., "the icon and label are centered together");
    };
    check_position();
    input.perform(serde_json::json!([{"point":[800., 500.]}]));
    save_snapshot(&w, 100, || directory.join("add-paper.png"));
    let bounds = row.compute_bounds(&w.surface).unwrap();
    let point = [bounds.x() + bounds.width() * 0.5, bounds.y() + bounds.height() * 0.5];
    input.perform(serde_json::json!([{"point":point}]));
    assert!(surface.state_flags().contains(gtk::StateFlags::PRELIGHT));
    let check_feedback = || {
        let snapshot = gtk::Snapshot::new();
        snapshot.render_background(&surface.style_context(), 0., 0., surface.width() as f64, surface.height() as f64);
        assert!(snapshot.to_node().unwrap().is::<gtk::gsk::ColorNode>(), "the feedback is a single square surface");
    };
    check_feedback();
    save_snapshot(&w, 100, || directory.join("add-paper-hover.png"));
    input.perform(serde_json::json!([{"down":true}]));
    assert!(surface.state_flags().contains(gtk::StateFlags::ACTIVE));
    check_feedback();
    save_snapshot(&w, 100, || directory.join("add-paper-active.png"));
    input.perform(serde_json::json!([{"point":[800., 500.]}, {"down":false}]));
    ready(&w);
    assert!(ui_session(&w).engine().document().bottom_paper().is_none(), "releasing outside cancels the action");
    assert!(!surface.state_flags().intersects(gtk::StateFlags::PRELIGHT | gtk::StateFlags::ACTIVE), "cancel clears hover and pressed feedback");
    input.perform(serde_json::json!([{"point":point}, {"down":true}, {"down":false}]));
    ready(&w);
    assert!(ui_session(&w).engine().document().bottom_paper().is_some());
    assert!(add().is_none());
    save_snapshot(&w, 100, || directory.join("paper-properties.png"));
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    let change = ui_session_mut(&w).reveal_panel(Panel::Layers);
    w.changed(change);
    ready(&w);
    assert!(add().is_some());
    check_position();
    let before = ui_session(&w).engine().checkpoint();
    let surface = add().unwrap().parent().unwrap();
    assert!(surface.grab_focus());
    input.key(0xff52);
    input.key(0xff54);
    assert!(surface.has_focus() && surface.state_flags().contains(gtk::StateFlags::FOCUS_VISIBLE));
    save_snapshot(&w, 100, || directory.join("add-paper-focus.png"));
    input.key(0xff0d);
    ready(&w);
    assert!(ui_session(&w).engine().document().bottom_paper().is_some(), "Enter activates the focused row");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    let change = ui_session_mut(&w).reveal_panel(Panel::Layers);
    w.changed(change);
    ready(&w);
    assert_eq!(ui_session(&w).engine().checkpoint(), before, "keyboard activation is one undo step");
    w.dispatch(UiAction::Invoke { command: CommandId::Redo });
    ready(&w);
    assert!(add().is_none());
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    for _ in 0..30 { w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::New { group: false, clipped: false } }); }
    ready(&w);
    let scroll = w.layer_panel.list.vadjustment();
    assert!(scroll.upper() > scroll.page_size());
    scroll.set_value(0.);
    pump(120);
    assert!(add().is_none_or(|row| row.compute_bounds(&w.layer_panel.list).is_none_or(|bounds| bounds.y() >= w.layer_panel.list.height() as f32)), "Add paper scrolls out of the viewport with the layers");
    scroll.set_value(scroll.upper() - scroll.page_size());
    until(|| add().is_some(), "the final list item is visible");
    pump(120);
    check_position();
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::SelectAllLayers { selected: true } });
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::DeleteSelected });
    ready(&w);
    assert!(state(&w).layers.is_empty());
    let bounds = add().unwrap().compute_bounds(&w.layer_panel.list).unwrap();
    assert!(bounds.y().abs() <= 2., "Add paper starts the empty layer list");
}

fn distribution(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values.get(((values.len() as f64 * q).ceil() as usize).saturating_sub(1)).copied();
    serde_json::json!({ "count": values.len(), "p50": at(0.5), "p95": at(0.95), "p99": at(0.99), "max": values.last() })
}

#[test]
#[ignore = "private Wayland display, hardware GPU and injected input"]
fn native_paper_scale_drag() {
    glib::set_prgname(Some("capy-canvas-test"));
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(60);
    let app = native_test_app("art.capycanvas.PaperScaleDrag");
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let project = layer_ui::NewDocumentOptions { extent: [4248, 2832], ..Default::default() }.project(&localization).unwrap();
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.maximize();
    w.window.present();
    ready(&w);
    let theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "default".into());
    let directory = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| "../../artifacts/paper-scale".into()));
    std::fs::create_dir_all(&directory).unwrap();
    let paper = layer_ui::occurrence_token(layer_core::paper::paper_occurrence(ui_session(&w).engine().document().scene(), None).unwrap());
    w.dispatch(UiAction::SelectLayer { id: paper });
    ready(&w);
    let control: crate::number_control::NumberControl = named(w.window.upcast_ref(), "property-scale");
    let slider = widgets(control.upcast_ref()).find_map(|widget| widget.downcast::<gtk::Scale>().ok()).expect("Scale has a slider");
    assert!(slider.is_mapped(), "the Scale slider is visible in Properties");
    let trough = widgets(slider.upcast_ref()).find(|widget| widget.css_name() == "trough").unwrap();
    let at = |position: f32| screen_point(&trough, &w.window, [position, 0.5]);
    let sweep = |from: f32, to: f32, events: usize| (1..=events).map(|i| serde_json::json!({ "point": at(from + (to - from) * i as f32 / events as f32) })).collect::<Vec<_>>();
    input.ready();
    pump(1500);
    let stats = ui_session(&w).engine().backend().stats.clone();
    let before = ui_session(&w).engine().checkpoint();
    let changes = Rc::new(std::cell::RefCell::new(Vec::<u64>::new()));
    let changed = slider.connect_value_changed(glib::clone!(#[strong] changes, move |_| changes.borrow_mut().push(glib::monotonic_time() as u64 * 1000)));
    let mut events = vec![serde_json::json!({ "point": at(0.5) }), serde_json::json!({ "down": true })];
    events.extend(sweep(0.5, 0.95, 120));
    events.extend(sweep(0.95, 0.05, 240));
    events.extend(sweep(0.05, 0.7, 160));
    events.push(serde_json::json!({ "down": false }));
    *stats.lock().unwrap() = Default::default();
    let start = glib::monotonic_time() as u64 * 1000;
    input.perform(serde_json::Value::Array(events));
    let end = glib::monotonic_time() as u64 * 1000;
    slider.disconnect(changed);
    assert!(ui_session(&w).engine().backend().has_paper_overlay(), "the relief shows while Scale moves");
    pump(2500);
    assert!(!ui_session(&w).engine().backend().has_paper_overlay(), "the relief hides after its own bake has shown");
    let report = {
        let stats = stats.lock().unwrap();
        let shown: Vec<_> = stats.presented.iter().filter(|p| p[3] == 1 && p[1] >= start).map(|p| p[1]).collect();
        let presented: Vec<_> = shown.iter().copied().filter(|p| *p <= end).collect();
        let latency: Vec<f64> = changes.borrow().iter().filter_map(|changed| {
            let applied = stats.paper_overlays.iter().copied().filter(|at| at >= changed).min()?;
            shown.iter().find(|p| **p >= applied).map(|p| (p - changed) as f64 / 1e6)
        }).collect();
        assert_eq!(latency.len(), changes.borrow().len(), "every Scale change reaches the canvas");
        serde_json::json!({
            "drag_ms": (end - start) as f64 / 1e6,
            "scale_changes": changes.borrow().len(),
            "slider_to_relief_ms": distribution(latency),
            "presented_fps": presented.len() as f64 / ((end - start) as f64 / 1e9),
            "presentation_gap_ms": distribution(presented.windows(2).map(|p| (p[1] - p[0]) as f64 / 1e6).collect()),
            "gpu_ms": distribution(stats.gpu.iter().map(|g| g[1]).collect()),
            "render_cpu_ms": distribution(stats.cpu.iter().map(|c| c[3]).collect()),
        })
    };
    std::fs::write(directory.join(format!("scale-drag-{theme}.json")), serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("paper scale drag: {report}");
    assert_ne!(ui_session(&w).engine().checkpoint(), before);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    ready(&w);
    assert_eq!(ui_session(&w).engine().checkpoint(), before, "a Scale drag is one undo step");

    let capture = |name: &str| match std::env::var_os("LAYER_NATIVE_CAPTURE_DIR") {
        Some(_) => serde_json::json!({ "capture": format!("{name}-{theme}") }),
        None => serde_json::json!({ "wait_ms": 50 }),
    };
    input.perform(serde_json::json!([{ "point": at(0.5) }, { "down": true }]));
    let mut small = sweep(0.5, 0.05, 60);
    small.push(capture("scale-dragging-small"));
    input.perform(serde_json::Value::Array(small));
    let mut large = sweep(0.05, 0.95, 90);
    large.push(capture("scale-dragging-large"));
    input.perform(serde_json::Value::Array(large));
    input.perform(serde_json::json!([{ "down": false }, { "wait_ms": 100 }, capture("scale-released-large")]));
    pump(2500);
    assert!(!ui_session(&w).engine().backend().has_paper_overlay());
    input.finish();
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_paper_lighting() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.PaperLighting");
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let project = layer_ui::NewDocumentOptions { extent: [1600, 1000], ..Default::default() }.project(&localization).unwrap();
    let w = Workspace::with_project(&app, Some((project, None)));
    apply_fixture_theme(&w);
    w.window.maximize();
    w.window.present();
    ready(&w);
    let mut input = RemoteInput::new();
    input.ready();
    let theme = std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "default".into());
    let directory = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| "../../artifacts/paper".into()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = |name: &str| directory.join(format!("{name}-{theme}.png"));
    w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Pencil as u32 });
    shade(&w, [180., 200.], 0.45);
    let paper = layer_ui::occurrence_token(layer_core::paper::paper_occurrence(ui_session(&w).engine().document().scene(), None).unwrap());
    w.dispatch(UiAction::SelectLayer { id: paper });
    w.dispatch(UiAction::Effect { action: layer_ui::EffectAction::Set { layer: paper, key: "texture".into(), value: EffectValue::Choice(1) } });
    ready(&w);
    pump(2000);
    let control = |key: &str| named::<crate::number_control::NumberControl>(w.window.upcast_ref(), &format!("property-{key}"));
    let (relief, light, through) = (control("relief"), control("light"), control("through_paint"));
    assert!(relief.is_sensitive() && !light.is_sensitive() && !through.is_sensitive(), "a flat paper has nothing to light");
    save_snapshot(&w, 200, || path("paper-lighting-flat"));
    let before = ui_session(&w).engine().checkpoint();
    edit_number(&relief, "150");
    until(|| light.is_sensitive() && through.is_sensitive(), "relief turns on the lighting controls");
    assert!(layer_core::paper::lit_paper(ui_session(&w).engine().document().scene()).is_some(), "the canvas shades the relief");
    pump(500);
    save_snapshot(&w, 200, || path("paper-lighting-lit"));
    let dial = find_css(light.upcast_ref(), "number-dial").unwrap();
    let metrics = layer_ui::PANEL_CONTROL_METRICS;
    let size = (metrics.slider_height - 2. * metrics.control_spacing) as i32;
    assert_eq!((dial.width(), dial.height()), (size, size), "the Light dial fits a slider row's height");
    assert_eq!(light.height(), relief.height(), "the Light row is as tall as a slider row");
    let end = |widget: &gtk::Widget| widget.compute_bounds(&w.window).map(|bounds| bounds.x() + bounds.width()).unwrap();
    let track = find_css(relief.upcast_ref(), "number-track").unwrap();
    assert!((end(&dial) - end(&track)).abs() < 1., "the dial ends where the sliders end: {} {}", end(&dial), end(&track));
    let lit = ui_session(&w).engine().checkpoint();
    input.click(screen_point(&dial, &w.window, [0.08, 0.5]));
    until(|| state(&w).layer_properties.controls.iter().any(|c| c.key == "light" && c.value == EffectValue::Number(180.)), "a press on the dial's left edge lights from the left");
    pump(300);
    save_snapshot(&w, 200, || path("paper-lighting-left"));
    let turned = ui_session(&w).engine().checkpoint();
    let scroller = through.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>().unwrap();
    let adjustment = scroller.vadjustment();
    adjustment.set_value(adjustment.upper() - adjustment.page_size());
    edit_number(&through, "0");
    until(|| layer_core::paper::separate_paper(ui_session(&w).engine().document().scene()).is_some_and(|(_, light)| light.through_paint == 0.),
        "paint hides the texture it covers");
    pump(500);
    save_snapshot(&w, 200, || path("paper-lighting-covered"));
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    pump(300);
    assert_eq!(ui_session(&w).engine().checkpoint(), turned, "hiding the texture under paint is one step");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    pump(300);
    assert_eq!(ui_session(&w).engine().checkpoint(), lit, "turning the light is one step");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    pump(300);
    assert_eq!(ui_session(&w).engine().checkpoint(), before, "raising the relief is one step");
    assert!(!light.is_sensitive());
    input.finish();
}
