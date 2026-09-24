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
    w.window.destroy();
    pump(100);
}
