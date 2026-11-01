use super::*;

fn includes_texture(node: &gtk::gsk::RenderNode, texture: &gtk::gdk::Texture) -> bool {
    node.downcast_ref::<gtk::gsk::TextureNode>().is_some_and(|node| node.texture() == *texture)
        || node.downcast_ref::<gtk::gsk::TextureScaleNode>().is_some_and(|node| node.texture() == *texture)
        || node.children().into_iter().any(|node| includes_texture(&node, texture))
}

pub(crate) fn texture_pixels(texture: &gdk::Texture) -> ([i32; 2], Vec<u8>) {
    let size = [texture.width(), texture.height()];
    let mut bytes = vec![0; size[0] as usize * size[1] as usize * 4];
    texture.download(&mut bytes, size[0] as usize * 4);
    (size, bytes)
}

pub(crate) struct PreviewPaints {
    clock: gdk::FrameClock,
    handler: glib::SignalHandlerId,
    renderer: gtk::gsk::Renderer,
    bounds: gtk::graphene::Rect,
    initial_texture: Option<gdk::Texture>,
    frames: Rc<RefCell<Vec<(i64, Option<gtk::gsk::RenderNode>, Option<gdk::Texture>)>>>,
}
impl PreviewPaints {
    pub(crate) fn start(widget: &gtk::Widget, texture: impl Fn() -> Option<gdk::Texture> + 'static) -> Self {
        let widget = widget.clone();
        let clock = widget.frame_clock().unwrap();
        let renderer = widget.native().unwrap().renderer().unwrap();
        let bounds = gtk::graphene::Rect::new(0., 0., widget.width() as f32, widget.height() as f32);
        let initial_texture = texture();
        let frames = Rc::new(RefCell::new(Vec::new()));
        let handler = clock.connect_after_paint(glib::clone!(#[strong] widget, #[strong] frames, move |clock| {
            let image = gtk::WidgetPaintable::new(Some(&widget)).current_image();
            let snapshot = gtk::Snapshot::new();
            image.snapshot(&snapshot, widget.width() as f64, widget.height() as f64);
            frames.borrow_mut().push((clock.frame_time(), snapshot.to_node(), texture()));
        }));
        Self { clock, handler, renderer, bounds, initial_texture, frames }
    }
    pub(crate) fn assert_nonempty(self, name: &str) {
        self.clock.disconnect(self.handler);
        let frames = self.frames.borrow();
        assert!(frames.len() > 1, "{name}: multiple actual paints during native drag");
        for (time, node, _) in frames.iter() {
            let node = node.as_ref().unwrap_or_else(|| panic!("{name}: blank preview at paint {time}"));
            let image = self.renderer.render_texture(node, Some(&self.bounds));
            let mut bytes = vec![0; image.width() as usize * image.height() as usize * 4];
            image.download(&mut bytes, image.width() as usize * 4);
            assert!(bytes.chunks_exact(4).any(|pixel| pixel[3] > 0), "{name}: empty preview pixels at paint {time}");
        }
    }
    pub(crate) fn shows(&self, texture: &gdk::Texture) -> bool {
        self.frames.borrow().last().and_then(|(_, node, _)| node.as_ref()).is_some_and(|node| includes_texture(node, texture))
    }
    pub(crate) fn assert_textures(self, name: &str) {
        self.clock.disconnect(self.handler);
        let frames = self.frames.borrow();
        assert!(!frames.is_empty(), "{name}: actual paint observed");
        let mut previous = self.initial_texture.as_ref();
        for (time, node, texture) in frames.iter() {
            let (node, texture) = node.as_ref().zip(texture.as_ref())
                .unwrap_or_else(|| panic!("{name}: blank preview at actual paint {time}"));
            let present = includes_texture(node, texture);
            let transitioning = previous.is_some_and(|old| old != texture && includes_texture(node, old));
            assert!(present || transitioning, "{name}: accepted preview texture missing from actual paint {time}");
            previous = Some(texture);
        }
    }
}

#[test]
#[ignore = "private GTK display"]
fn native_color_fields_keep_completed_pixels_during_edits() {
    use gtk::subclass::prelude::ObjectSubclassIsExt;
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
        let paints = PreviewPaints::start(wheel.upcast_ref(), glib::clone!(#[strong] wheel, move ||
            wheel.imp().disc.borrow().as_ref().map(|disc| disc.7.clone())
        ));
        let arc_paints = wheel.intensity().filter(|arc| arc.is_visible()).map(|arc|
            PreviewPaints::start(arc.upcast_ref(), glib::clone!(#[strong] arc, move ||
                arc.imp().texture.borrow().as_ref().map(|(_, texture)| texture.clone())
            ))
        );
        wheel.set_state(&current, view, 1., false, None);
        super::super::color_panel::settled_wheel(&wheel);
        let final_field = wheel.imp().disc.borrow().as_ref().unwrap().7.clone();
        assert_ne!(final_field, old_field, "settled hue uses its exact current field");
        until(|| paints.shows(&final_field), "current color field reaches an actual paint");
        if let (Some(paints), Some(arc)) = (arc_paints, wheel.intensity().filter(|arc| arc.is_visible())) {
            if let Some(old) = old_arc {
                until(|| arc.imp().texture.borrow().as_ref().is_some_and(|(_, texture)| *texture != old),
                    "intensity arc finishes its current base color");
            }
            let texture = arc.imp().texture.borrow().as_ref().unwrap().1.clone();
            until(|| paints.shows(&texture), "current intensity arc reaches an actual paint");
            paints.assert_textures("pending intensity arc retains accepted pixels");
        }
        paints.assert_textures("pending color field retains accepted pixels");
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
    let mut d = Driver::new("art.capycanvas.ThumbnailFreshness");
    d.w.customize(CustomizationAction::SetPanelVisible { panel: Panel::Layers, visible: true });
    d.w.dispatch(UiAction::MovePanel { panel: Panel::Layers,
        target: DockTarget::Edge { edge: Edge::Left, outer: false },
        viewport: [d.w.window.width() as f32, 800.] });
    new_photo::ready(&d.w);
    until(|| d.w.layer_panel.root.is_mapped(), "explicitly docked Layers panel mapped");
    let id = state(&d.w).layers.iter().find(|layer|
        layer.selected && !layer.group && layer.has_thumbnail && layer.fill_color.is_none() && !layer.selection_layer
    ).expect("selected paint source").id;
    let blank = super::place_source::wait_layer_thumbnail(&d.w, id);
    let blank_pixel = thumbnail_freshness_center(&blank);
    let row = named::<gtk::Box>(d.w.layer_panel.root.upcast_ref(), &format!("art-layer-{id}"));
    let thumbnail = find_css(row.upcast_ref(), "layer-thumbnail").expect("content thumbnail button");
    let picture = descendant::<gtk::Picture>(&thumbnail).expect("actual content Picture");
    until(|| picture.paintable().is_some(), "first layer thumbnail accepted");
    let paints = PreviewPaints::start(picture.upcast_ref(), glib::clone!(#[strong] picture, move ||
        picture.paintable().and_downcast::<gdk::Texture>()
    ));
    d.w.dispatch(UiAction::SetColor { rgba: [1., 0., 0., 1.] });
    d.w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::GPen as u32 });
    d.w.dispatch(UiAction::SetBrushSize { value: 128. });
    d.w.dispatch(UiAction::SetBrushStrength { value: 1. });
    let at = |fraction| screen_point(d.w.area.upcast_ref(), &d.w.window, [fraction, 0.5]);
    let mut events = vec![serde_json::json!({"point":at(0.35),"down":true})];
    events.extend((0..32).map(|i| serde_json::json!({"point":at(0.35 + 0.3 * i as f32 / 31.)})));
    events.push(serde_json::json!({"down":false}));
    d.input.perform(serde_json::Value::Array(events));
    let red = super::place_source::wait_layer_thumbnail(&d.w, id);
    let red_pixel = thumbnail_freshness_center(&red);
    assert!(red_pixel[0] > 180 && red_pixel[1] < 100 && red_pixel[2] < 100, "actual stroke thumbnail is red: {red_pixel:?}");
    assert_ne!(blank_pixel, red_pixel, "nonvacuous stroke and blank references");
    until(|| paints.shows(&red), "stroke thumbnail reaches an actual paint");
    let previous_revision = state(&d.w).layers.iter().find(|layer| layer.id == id).unwrap().paint_revision;
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    let current_revision = state(&d.w).layers.iter().find(|layer| layer.id == id).unwrap().paint_revision;
    assert_ne!(current_revision, previous_revision, "Undo publishes its current source revision");
    let restored = super::place_source::wait_layer_thumbnail(&d.w, id);
    assert_eq!(texture_pixels(&restored), texture_pixels(&blank), "exact current GPU thumbnail restores the blank source");
    until(|| paints.shows(&restored), "current Undo thumbnail reaches an actual paint");
    paints.assert_textures("layer thumbnail stays present through stroke and Undo");
    d.finish();
}
