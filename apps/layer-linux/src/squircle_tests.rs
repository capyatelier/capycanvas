use super::*;

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
    let converted = crate::squircle::converted(shadowed.upcast_ref());
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
        let converted = crate::squircle::converted(drawer_shadow.upcast_ref());
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
