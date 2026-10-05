use super::*;

#[test]
#[ignore = "private Mutter: --native-test=native_layer_thumbnail_squircles"]
fn native_layer_thumbnail_squircles() {
    let app = native_test_app("art.capycanvas.LayerThumbnailSquircles");
    let mut project = new_drawing(128, 64, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    active_paint_mut(&mut project).original = Some(std::sync::Arc::new(place_source::source()));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.area.connect_realize(glib::clone!(#[weak] w, move |_| w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(layer_ui::WorkspaceState::default()),
    })));
    w.window.maximize();
    w.window.present();
    new_photo::ready(&w);
    let paint = state(&w).layer_tools.editing_layer.unwrap().id;
    place_source::wait_layer_thumbnail(&w, paint);
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Rename { id: paint, name: "Color study".into() } });
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::AddMask { id: paint, replace: false } });
    w.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "gradient_fill".into() } });
    let fill = state(&w).layer_tools.editing_layer.unwrap().id;
    w.dispatch(UiAction::SelectPanelTab { group: state(&w).workspace.layout.panel_group(Panel::Layers).unwrap(), panel: Panel::Layers });
    place_source::wait_layer_thumbnail(&w, fill);
    pump(250);
    let row = widgets(w.layer_panel.root.upcast_ref()).find(|widget| widget.is_mapped() && widget.widget_name() == format!("art-layer-{paint}")).unwrap();
    let thumbnails = descendants::<gtk::Button>(&row).into_iter().filter(|button| button.has_css_class("layer-thumbnail")).collect::<Vec<_>>();
    assert_eq!(thumbnails.len(), 2);
    let render = |widget: &gtk::Widget| {
        let snapshot = gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(widget)).snapshot(&snapshot, widget.width() as f64, widget.height() as f64);
        w.window.renderer().unwrap().render_texture(
            &crate::squircle::converted(&snapshot.to_node().unwrap(), 1.),
            Some(&gtk::graphene::Rect::new(0., 0., widget.width() as f32, widget.height() as f32)),
        )
    };
    let mut input = RemoteInput::new();
    input.ready();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        for (mask, button) in thumbnails.iter().enumerate() {
            let picture = descendant::<gtk::Picture>(button).unwrap();
            until(|| picture.paintable().is_some(), "thumbnail pixels published");
            let texture = render(picture.upcast_ref());
            assert_eq!([texture.width(), texture.height()], [28, 28]);
            let mut download = gdk::TextureDownloader::new(&texture);
            download.set_format(gdk::MemoryFormat::R8g8b8a8);
            let (pixels, stride) = download.download_bytes();
            let alpha = |x: usize, y: usize| pixels[y * stride + x * 4 + 3];
            for (x, y) in [(0, 0), (27, 0), (0, 27), (27, 27)] {
                assert_eq!(alpha(x, y), 0, "{theme:?}/{mask}: preview corners are clipped");
            }
            assert_eq!(alpha(3, 3), 255, "the squircle keeps pixels outside an inscribed circle");
            assert_eq!(alpha(14, 14), 255);
            let bounds = button.compute_bounds(&w.surface).unwrap();
            input.click([bounds.x() + 3., bounds.y() + 3.]);
            assert_eq!(state(&w).layer_tools.editing_layer.unwrap().mask_selected, mask == 1);
            input.perform(serde_json::json!([{"point":screen_point(w.area.upcast_ref(), &w.surface, [0.5, 0.2])}]));
            assert!(button.has_css_class("layer-editing"));
            let active = render(button.upcast_ref());
            let mut download = gdk::TextureDownloader::new(&active);
            download.set_format(gdk::MemoryFormat::R8g8b8a8);
            let (active_pixels, active_stride) = download.download_bytes();
            let border = button.height() as usize / 2 * active_stride;
            let expected = state(&w).palette.accent.0;
            for channel in 0..3 {
                assert!(active_pixels[border + channel].abs_diff(expected[channel]) <= 1, "the outer edge uses the accent color");
            }
            let origin = picture.compute_bounds(button).unwrap();
            for (x, y) in [(1usize, 14usize), (26, 14), (14, 1), (14, 26)] {
                let source = y * stride + x * 4;
                let target = (origin.y() as usize + y) * active_stride + (origin.x() as usize + x) * 4;
                for channel in 0..4 {
                    assert!(active_pixels[target + channel].abs_diff(pixels[source + channel]) <= 1, "the active border leaves preview edges unchanged at {x},{y}");
                }
            }
            if let Some(directory) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                let name = format!("{}-{}", format!("{theme:?}").to_lowercase(), if mask == 1 { "mask" } else { "content" });
                render(w.layer_panel.root.upcast_ref()).save_to_png(directory.join(format!("panel-{name}.png"))).unwrap();
                if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
                    input.perform(serde_json::json!([{"capture":name}]));
                }
            }
        }
        let preview = crate::layers::drag_preview(&row, state(&w).palette.panel).unwrap();
        let snapshot = gtk::Snapshot::new();
        preview.snapshot(&snapshot, row.width() as f64, row.height() as f64);
        let texture = w.window.renderer().unwrap().render_texture(&snapshot.to_node().unwrap(), None);
        if let Some(directory) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
            texture.save_to_png(std::path::PathBuf::from(directory).join(format!("drag-{}.png", format!("{theme:?}").to_lowercase()))).unwrap();
        }
    }
    input.finish();
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "private Mutter: --native-test=native_squircle_corners"]
fn native_squircle_corners() {
    let app = native_test_app("art.capycanvas.SquircleCorners");
    let w = fixture_workspace(&app);
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(20);
    let tile = loop {
        pump(50);
        let tile = w.toolbar.first_child().filter(|t| t.is_mapped() && t.width() == TILE_SIZE as i32);
        if let Some(tile) = tile {
            break tile;
        }
        assert!(Instant::now() < deadline, "toolbar tiles did not map");
    };
    assert!(tile.contains(3., 3.), "painted squircle corners hit their tile");
    let b = tile.compute_bounds(&w.window).unwrap();
    let picked = w
        .window
        .pick(f64::from(b.x()) + 3., f64::from(b.y()) + 3., gtk::PickFlags::DEFAULT)
        .unwrap();
    assert!(picked == tile || picked.is_ancestor(&tile), "corner pick reaches the tile: {picked:?}");

    let rect = gtk::graphene::Rect::new(0., 0., 36., 36.);
    let fill = gtk::gsk::RoundedClipNode::new(
        gtk::gsk::ColorNode::new(&gdk::RGBA::WHITE, &rect),
        &gtk::gsk::RoundedRect::from_rect(rect, 18.),
    );
    let shadowed = gtk::gsk::ShadowNode::new(&fill, &[gtk::gsk::Shadow::new(gdk::RGBA::BLACK, 0., 8., 24.)]);
    let converted = crate::squircle::converted(shadowed.upcast_ref(), 1.);
    let shadow = converted.downcast_ref::<gtk::gsk::ShadowNode>().expect("shadow is preserved");
    assert_eq!(shadow.n_shadows(), 1);
    assert!(shadow.child().is::<gtk::gsk::MaskNode>(), "shadowed contents become squircles");

    let corner = gtk::graphene::Size::new(9.72, 9.72);
    let float = gdk::MemoryTexture::new(1, 1, gdk::MemoryFormat::R32g32b32a32Float, &glib::Bytes::from_owned(vec![0u8; 16]), 16);
    for [width, height] in [[272., 300.], [40., 40.]] {
        let outline = gtk::gsk::RoundedRect::new(
            gtk::graphene::Rect::new(0., 0., width, height),
            gtk::graphene::Size::new(0., 0.),
            corner,
            corner,
            corner,
        );
        let drawer_shadow = gtk::gsk::OutsetShadowNode::new(&outline, &gdk::RGBA::new(0., 0., 0., 0.4), 0., 8., 0., 24.);
        let converted = crate::squircle::converted(drawer_shadow.upcast_ref(), 1.);
        assert!(!converted.is::<gtk::gsk::OutsetShadowNode>(), "blurred squircle shadows bypass GTK's box-shadow shader");
        let snapshot = gtk::Snapshot::new();
        snapshot.append_texture(&float, &gtk::graphene::Rect::new(0., 0., 1., 1.));
        snapshot.append_node(&converted);
        let texture = w.window.renderer().unwrap().render_texture(snapshot.to_node().unwrap(), Some(&converted.bounds()));
        let mut download = gdk::TextureDownloader::new(&texture);
        download.set_format(gdk::MemoryFormat::R32g32b32a32Float);
        let (pixels, _) = download.download_bytes();
        let values: Vec<f32> = pixels.chunks_exact(4).map(|c| f32::from_ne_bytes(c.try_into().unwrap())).collect();
        assert!(values.iter().all(|v| v.is_finite()), "{width}x{height} drawer shadow pixels stay finite");
        assert!(values.chunks_exact(4).any(|p| p[3] > 0.1), "{width}x{height} drawer shadow is painted");
    }
    w.window.destroy();
    pump(100);
}
