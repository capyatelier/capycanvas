use super::*;

fn includes_texture(node: &gtk::gsk::RenderNode, texture: &gtk::gdk::Texture) -> bool {
    node.downcast_ref::<gtk::gsk::TextureNode>().is_some_and(|node| node.texture() == *texture)
        || node.downcast_ref::<gtk::gsk::TextureScaleNode>().is_some_and(|node| node.texture() == *texture)
        || node.children().into_iter().any(|node| includes_texture(&node, texture))
}

#[test]
#[ignore = "private GTK display"]
fn native_color_fields_withhold_obsolete_cached_pixels() {
    use gtk::subclass::prelude::{ObjectSubclassIsExt, WidgetImpl};
    let app = native_test_app("art.capycanvas.CachedColorFields");
    for hdr in [false, true] {
        let window: crate::window::Window = glib::Object::builder().property("application", &*app)
            .property("default-width", 280).property("default-height", 360).build();
        adw::StyleManager::default().set_color_scheme(match std::env::var("CAPY_NATIVE_TEST_THEME").as_deref() {
            Ok("light") => adw::ColorScheme::ForceLight, _ => adw::ColorScheme::ForceDark,
        });
        let wheel = crate::tool_panels::ColorWheel::bare();
        let mut old = layer_ui::ColorState::default();
        old.set_color(layer_core::color::RgbColor::from_linear(layer_core::color::RgbSpace::Srgb, [1.,0.,0.,1.]).unwrap()).unwrap();
        old.set_hdr_enabled(hdr).unwrap();
        let view = if hdr { crate::display_color::ViewColor::Srgb.with_rendition(layer_core::color::DocumentColor {
            depth: layer_core::color::SampleDepth::F16, ..Default::default()
        }, Default::default()) } else { crate::display_color::ViewColor::Srgb };
        wheel.set_state(&old, view, 1., false, None);
        window.set_content(Some(&wheel));
        window.present();
        until(|| wheel.is_mapped(), "color field mapped");
        super::super::color_panel::settled_wheel(&wheel);
        let old_field = wheel.imp().disc.borrow().as_ref().unwrap().7.clone();
        let old_arc = wheel.intensity().filter(|arc| arc.is_visible())
            .and_then(|arc| arc.imp().texture.borrow().as_ref().map(|(_, texture)| texture.clone()));
        let mut current = old.clone();
        current.set_color(layer_core::color::RgbColor::from_linear(layer_core::color::RgbSpace::Srgb, [0.,0.,1.,1.]).unwrap()).unwrap();
        wheel.set_state(&current, view, 1., false, None);
        let snapshot = gtk::Snapshot::new();
        wheel.imp().snapshot(&snapshot);
        let node = snapshot.to_node().unwrap();
        assert!(!includes_texture(&node, &old_field), "new hue must not paint the previous field");
        if let Some(old_arc) = old_arc {
            let arc = wheel.intensity().unwrap();
            let snapshot = gtk::Snapshot::new();
            arc.imp().snapshot(&snapshot);
            assert!(!snapshot.to_node().is_some_and(|node| includes_texture(&node, &old_arc)),
                "new base color must not paint the previous intensity arc");
        }
        let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
        window.renderer().unwrap().render_texture(&node, None)
            .save_to_png(output.join(if hdr { "hdr-field-pending.png" } else { "sdr-field-pending.png" })).unwrap();
        super::super::color_panel::settled_wheel(&wheel);
        window.destroy();
        until(|| !window.is_realized(), "color field window retired");
    }
}

fn thumbnail_freshness_center(texture: &gtk::gdk::Texture) -> [u8; 4] {
    let mut download = gtk::gdk::TextureDownloader::new(texture);
    download.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = download.download_bytes();
    let offset = texture.height() as usize / 2 * stride + texture.width() as usize / 2 * 4;
    bytes.as_ref()[offset..offset + 4].try_into().unwrap()
}

#[test]
#[ignore = "private display and hardware GPU"]
fn native_layer_thumbnail_old_pixels_after_undo() {
    let d = Driver::new("art.capycanvas.ThumbnailFreshness");
    let id = state(&d.w).layers.iter().find(|layer|
        layer.selected && !layer.group && layer.has_thumbnail && layer.fill_color.is_none() && !layer.selection_layer
    ).expect("selected paint source").id;
    let blank = super::place_source::wait_layer_thumbnail(&d.w, id);
    let blank_pixel = thumbnail_freshness_center(&blank);
    d.w.dispatch(UiAction::SetColor { rgba: [1., 0., 0., 1.] });
    d.w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::FillSelection });
    let red = super::place_source::wait_layer_thumbnail(&d.w, id);
    let red_pixel = thumbnail_freshness_center(&red);
    assert!(red_pixel[0] > 180 && red_pixel[1] < 100 && red_pixel[2] < 100,
        "real GPU thumbnail A is red: {red_pixel:?}");
    assert_ne!(blank_pixel, red_pixel, "nonvacuous old/current pixel references");
    let previous_revision = state(&d.w).layers.iter().find(|layer| layer.id == id).unwrap().paint_revision;

    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    let current_revision = state(&d.w).layers.iter().find(|layer| layer.id == id).unwrap().paint_revision;
    assert_ne!(current_revision, previous_revision, "Undo publishes a different current source revision synchronously");
    let current = state(&d.w).layers.iter().find(|layer| layer.id == id).unwrap().clone();
    let lookup = d.w.layer_panel.preview(&current);
    let lookup_pixel = lookup.as_ref().map(thumbnail_freshness_center);
    let row = d.named(&format!("art-layer-{id}"));
    let thumbnail = find_css(&row, "layer-thumbnail").expect("content thumbnail button");
    let picture = descendant::<gtk::Picture>(&thumbnail).expect("actual content Picture");
    let selected = picture.paintable().and_downcast::<gtk::gdk::Texture>();
    let selected_pixel = selected.as_ref().map(thumbnail_freshness_center);

    let restored = super::place_source::wait_layer_thumbnail(&d.w, id);
    let restored_pixel = thumbnail_freshness_center(&restored);
    assert_eq!(restored_pixel, blank_pixel, "actual current GPU thumbnail B restores the blank source");
    let lookup_is_old = lookup_pixel == Some(red_pixel);
    let selected_is_old = selected_pixel == Some(red_pixel);
    let reproduced = lookup_is_old || selected_is_old;
    let report = serde_json::json!({
        "protocol": 1,
        "case": "red-fill-then-undo",
        "layer": id,
        "previous_revision": previous_revision,
        "current_revision": current_revision,
        "old_gpu_pixel": red_pixel,
        "initial_blank_gpu_pixel": blank_pixel,
        "current_gpu_pixel": restored_pixel,
        "production_preview_lookup_pixel_at_undo": lookup_pixel,
        "actual_picture_pixel_at_undo": selected_pixel,
        "production_lookup_returned_old_pixels": lookup_is_old,
        "actual_picture_selected_old_pixels": selected_is_old,
        "reproduced": reproduced,
    });
    let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").expect("LAYER_TEST_ARTIFACTS"));
    std::fs::create_dir_all(&output).unwrap();
    red.save_to_png(output.join("thumbnail-old-red.png")).unwrap();
    restored.save_to_png(output.join("thumbnail-current-blank.png")).unwrap();
    std::fs::write(output.join("thumbnail-freshness.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    d.w.window.destroy();
    assert!(!reproduced, "current layer revision displayed an older thumbnail");
}
