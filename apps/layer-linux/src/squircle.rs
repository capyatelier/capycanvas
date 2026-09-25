//! Superellipse corners matching CSS `corner-shape: squircle` for the drawing interface.
use gtk::{cairo, gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::FRAC_PI_2;

pub(crate) const KEEP_ROUND: &str = "capy-keep-round";
const CORNER_SEGMENTS: u32 = 24;
const MASK_PRUNE_THRESHOLD: usize = 512;
pub const CORNER_FIT: f32 = 0.54;

pub fn append_round(snapshot: &gtk::Snapshot, round: gtk::Snapshot) {
    if let Some(node) = round.to_node() {
        snapshot.append_node(gsk::DebugNode::new(node, KEEP_ROUND.into()));
    }
}

fn corner(cr: &cairo::Context, center: [f64; 2], start: [f64; 2], end: [f64; 2]) {
    for step in 1..=CORNER_SEGMENTS {
        let angle = f64::from(step) / f64::from(CORNER_SEGMENTS) * FRAC_PI_2;
        let (along_start, along_end) = (angle.cos().sqrt(), angle.sin().sqrt());
        cr.line_to(
            center[0] + start[0] * along_start + end[0] * along_end,
            center[1] + start[1] * along_start + end[1] * along_end,
        );
    }
}

pub fn concave_foot(cr: &cairo::Context, x: f64, y: f64, radius: f64, direction: f64) {
    cr.move_to(x, y - radius);
    cr.line_to(x, y);
    cr.line_to(x + direction * radius, y);
    corner(cr, [x + direction * radius, y - radius], [0., radius], [-direction * radius, 0.]);
    cr.close_path();
}

pub fn rounded_rect(cr: &cairo::Context, rect: &gsk::RoundedRect) {
    let bounds = rect.bounds();
    let [left, top] = [f64::from(bounds.x()), f64::from(bounds.y())];
    let [right, bottom] = [left + f64::from(bounds.width()), top + f64::from(bounds.height())];
    let [top_left, top_right, bottom_right, bottom_left] =
        rect.corner().map(|size| [f64::from(size.width()), f64::from(size.height())]);
    cr.move_to(left + top_left[0], top);
    cr.line_to(right - top_right[0], top);
    corner(cr, [right - top_right[0], top + top_right[1]], [0., -top_right[1]], [top_right[0], 0.]);
    cr.line_to(right, bottom - bottom_right[1]);
    corner(
        cr,
        [right - bottom_right[0], bottom - bottom_right[1]],
        [bottom_right[0], 0.],
        [0., bottom_right[1]],
    );
    cr.line_to(left + bottom_left[0], bottom);
    corner(cr, [left + bottom_left[0], bottom - bottom_left[1]], [0., bottom_left[1]], [-bottom_left[0], 0.]);
    cr.line_to(left, top + top_left[1]);
    corner(cr, [left + top_left[0], top + top_left[1]], [-top_left[0], 0.], [0., -top_left[1]]);
    cr.close_path();
}

fn squircle_rect(circular: &gsk::RoundedRect) -> gsk::RoundedRect {
    let [top_left, top_right, bottom_right, bottom_left] = circular
        .corner()
        .map(|size| graphene::Size::new(size.width() / CORNER_FIT, size.height() / CORNER_FIT));
    let mut rect = gsk::RoundedRect::new(*circular.bounds(), top_left, top_right, bottom_right, bottom_left);
    rect.normalize();
    rect
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MaskKey([u32; 25]);

impl MaskKey {
    fn new(scale: f64, outer: &gsk::RoundedRect, inner: Option<&gsk::RoundedRect>) -> Self {
        let origin = outer.bounds();
        let geometry = |rect: &gsk::RoundedRect| {
            let bounds = rect.bounds();
            let [tl, tr, br, bl] = rect.corner();
            [
                bounds.x() - origin.x(),
                bounds.y() - origin.y(),
                bounds.width(),
                bounds.height(),
                tl.width(),
                tl.height(),
                tr.width(),
                tr.height(),
                br.width(),
                br.height(),
                bl.width(),
                bl.height(),
            ]
        };
        let values = std::iter::once(scale as f32)
            .chain(geometry(outer))
            .chain(inner.map(geometry).into_iter().flatten());
        let mut key = [0; 25];
        for (slot, value) in key.iter_mut().zip(values) {
            *slot = value.to_bits();
        }
        Self(key)
    }
}

fn rasterize(scale: f64, outer: &gsk::RoundedRect, inner: Option<&gsk::RoundedRect>) -> gdk::Texture {
    let bounds = outer.bounds();
    let width = (f64::from(bounds.width()) * scale).ceil().max(1.) as i32;
    let height = (f64::from(bounds.height()) * scale).ceil().max(1.) as i32;
    let mut surface = cairo::ImageSurface::create(cairo::Format::A8, width, height)
        .expect("allocate corner mask");
    {
        let cr = cairo::Context::new(&surface).expect("draw corner mask");
        cr.scale(scale, scale);
        cr.translate(-f64::from(bounds.x()), -f64::from(bounds.y()));
        rounded_rect(&cr, outer);
        let _ = cr.fill();
        if let Some(inner) = inner {
            cr.set_operator(cairo::Operator::Clear);
            rounded_rect(&cr, inner);
            let _ = cr.fill();
        }
    }
    let stride = surface.stride() as usize;
    let pixels = surface.data().expect("read corner mask").to_vec();
    gdk::MemoryTexture::new(width, height, gdk::MemoryFormat::A8, &glib::Bytes::from_owned(pixels), stride)
        .upcast()
}

#[derive(Default)]
struct Converter {
    scale: f64,
    nodes: HashMap<usize, (gsk::RenderNode, gsk::RenderNode)>,
    previous: HashMap<usize, (gsk::RenderNode, gsk::RenderNode)>,
    masks: HashMap<MaskKey, glib::WeakRef<gdk::Texture>>,
}

impl Converter {
    fn frame(&mut self, node: &gsk::RenderNode, scale: f64) -> gsk::RenderNode {
        if self.scale != scale {
            *self = Self { scale, ..Self::default() };
        }
        self.previous = std::mem::take(&mut self.nodes);
        if self.masks.len() > MASK_PRUNE_THRESHOLD {
            self.masks.retain(|_, mask| mask.upgrade().is_some());
        }
        let converted = self.convert(node);
        self.previous.clear();
        converted
    }

    fn convert(&mut self, node: &gsk::RenderNode) -> gsk::RenderNode {
        let key = node.as_ptr() as usize;
        if let Some((_, converted)) = self.nodes.get(&key) {
            return converted.clone();
        }
        let converted = match self.previous.remove(&key) {
            Some((_, converted)) => converted,
            None => self.rewrite(node),
        };
        self.nodes.insert(key, (node.clone(), converted.clone()));
        converted
    }

    fn rebuild(
        &mut self,
        node: &gsk::RenderNode,
        child: &gsk::RenderNode,
        wrap: impl FnOnce(&gsk::RenderNode) -> gsk::RenderNode,
    ) -> gsk::RenderNode {
        let converted = self.convert(child);
        if converted.as_ptr() == child.as_ptr() {
            node.clone()
        } else {
            wrap(&converted)
        }
    }

    fn rewrite(&mut self, node: &gsk::RenderNode) -> gsk::RenderNode {
        if let Some(container) = node.downcast_ref::<gsk::ContainerNode>() {
            let children: Vec<_> = (0..container.n_children()).map(|i| container.child(i)).collect();
            let converted: Vec<_> = children.iter().map(|child| self.convert(child)).collect();
            return if children.iter().zip(&converted).all(|(a, b)| a.as_ptr() == b.as_ptr()) {
                node.clone()
            } else {
                gsk::ContainerNode::new(&converted).upcast()
            };
        }
        if let Some(transform) = node.downcast_ref::<gsk::TransformNode>() {
            let matrix = transform.transform();
            return self.rebuild(node, &transform.child(), |child| {
                gsk::TransformNode::new(child, Some(&matrix)).upcast()
            });
        }
        if let Some(clip) = node.downcast_ref::<gsk::ClipNode>() {
            let rect = clip.clip();
            return self.rebuild(node, &clip.child(), |child| gsk::ClipNode::new(child, &rect).upcast());
        }
        if let Some(opacity) = node.downcast_ref::<gsk::OpacityNode>() {
            let alpha = opacity.opacity();
            return self.rebuild(node, &opacity.child(), |child| gsk::OpacityNode::new(child, alpha).upcast());
        }
        if let Some(isolation) = node.downcast_ref::<gsk::IsolationNode>() {
            let isolations = isolation.isolations();
            return self.rebuild(node, &isolation.child(), |child| {
                gsk::IsolationNode::new(child, isolations).upcast()
            });
        }
        if let Some(shadow) = node.downcast_ref::<gsk::ShadowNode>() {
            let shadows: Vec<_> = (0..shadow.n_shadows()).map(|i| shadow.shadow(i)).collect();
            return self.rebuild(node, &shadow.child(), |child| gsk::ShadowNode::new(child, &shadows).upcast());
        }
        if let Some(debug) = node.downcast_ref::<gsk::DebugNode>() {
            let message = debug.message();
            if message == KEEP_ROUND {
                return node.clone();
            }
            return self.rebuild(node, &debug.child(), |child| gsk::DebugNode::new(child, message).upcast());
        }
        if let Some(rounded) = node.downcast_ref::<gsk::RoundedClipNode>() {
            let clip = rounded.clip();
            if clip.is_rectilinear() {
                return self.rebuild(node, &rounded.child(), |child| {
                    gsk::RoundedClipNode::new(child, &clip).upcast()
                });
            }
            let child = self.convert(&rounded.child());
            return gsk::MaskNode::new(child, self.mask(&squircle_rect(&clip), None), gsk::MaskMode::Alpha).upcast();
        }
        if let Some(border) = node.downcast_ref::<gsk::BorderNode>() {
            let outline = border.outline();
            let colors = border.colors();
            if outline.is_rectilinear() || colors.iter().any(|color| color != &colors[0]) {
                return node.clone();
            }
            let [top, right, bottom, left] = *border.widths();
            let outer = squircle_rect(&outline);
            let mut inner = outer;
            inner.shrink(top, right, bottom, left);
            return self.ring(&outer, &inner, &colors[0]);
        }
        if let Some(shadow) = node.downcast_ref::<gsk::OutsetShadowNode>() {
            let outline = shadow.outline();
            if shadow.blur_radius() > 0. || outline.is_rectilinear() {
                return node.clone();
            }
            let spread = shadow.spread();
            let inner = squircle_rect(&outline);
            let mut outer = inner;
            outer.offset(shadow.dx(), shadow.dy());
            outer.shrink(-spread, -spread, -spread, -spread);
            return self.ring(&outer, &inner, &shadow.color());
        }
        if let Some(shadow) = node.downcast_ref::<gsk::InsetShadowNode>() {
            let outline = shadow.outline();
            if shadow.blur_radius() > 0. || outline.is_rectilinear() {
                return node.clone();
            }
            let spread = shadow.spread();
            let outer = squircle_rect(&outline);
            let mut inner = outer;
            inner.offset(shadow.dx(), shadow.dy());
            inner.shrink(spread, spread, spread, spread);
            return self.ring(&outer, &inner, &shadow.color());
        }
        node.clone()
    }

    fn ring(&mut self, outer: &gsk::RoundedRect, inner: &gsk::RoundedRect, color: &gdk::RGBA) -> gsk::RenderNode {
        let fill = gsk::ColorNode::new(color, outer.bounds());
        gsk::MaskNode::new(fill, self.mask(outer, Some(inner)), gsk::MaskMode::Alpha).upcast()
    }

    fn mask(&mut self, outer: &gsk::RoundedRect, inner: Option<&gsk::RoundedRect>) -> gsk::RenderNode {
        let key = MaskKey::new(self.scale, outer, inner);
        let texture = match self.masks.get(&key).and_then(|mask| mask.upgrade()) {
            Some(texture) => texture,
            None => {
                let texture = rasterize(self.scale, outer, inner);
                self.masks.insert(key, texture.downgrade());
                texture
            }
        };
        let bounds = outer.bounds();
        let scale = self.scale as f32;
        let area = graphene::Rect::new(
            bounds.x(),
            bounds.y(),
            texture.width() as f32 / scale,
            texture.height() as f32 / scale,
        );
        gsk::TextureNode::new(&texture, &area).upcast()
    }
}

#[cfg(test)]
pub(crate) fn converted(node: &gsk::RenderNode) -> gsk::RenderNode {
    Converter::default().frame(node, 1.)
}

fn append_converted(widget: &gtk::Widget, converter: &RefCell<Converter>, content: gtk::Snapshot, snapshot: &gtk::Snapshot) {
    let Some(node) = content.to_node() else {
        return;
    };
    let scale = widget
        .native()
        .and_then(|native| native.surface())
        .map_or_else(|| f64::from(widget.scale_factor()), |surface| surface.scale());
    snapshot.append_node(converter.borrow_mut().frame(&node, scale));
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Squircles {
        pub(super) converter: RefCell<Converter>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Squircles {
        const NAME: &'static str = "CapySquircles";
        type Type = super::Squircles;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for Squircles {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Squircles {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(child) = obj.first_child() else {
                return;
            };
            let content = gtk::Snapshot::new();
            obj.snapshot_child(&child, &content);
            append_converted(obj.upcast_ref(), &self.converter, content, snapshot);
        }
    }

    #[derive(Default)]
    pub struct Popover {
        pub(super) converter: RefCell<Converter>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Popover {
        const NAME: &'static str = "CapySquirclePopover";
        type Type = super::Popover;
        type ParentType = gtk::Popover;
    }

    impl ObjectImpl for Popover {}

    impl WidgetImpl for Popover {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let content = gtk::Snapshot::new();
            self.parent_snapshot(&content);
            append_converted(self.obj().upcast_ref(), &self.converter, content, snapshot);
        }
    }

    impl PopoverImpl for Popover {}
}

glib::wrapper! {
    pub struct Squircles(ObjectSubclass<imp::Squircles>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Squircles {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let this: Self = glib::Object::new();
        child.set_parent(&this);
        this
    }
}

glib::wrapper! {
    pub struct Popover(ObjectSubclass<imp::Popover>) @extends gtk::Popover, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::ShortcutManager;
}

impl Popover {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
