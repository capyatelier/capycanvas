//! Superellipse corners matching CSS `corner-shape: squircle` for the drawing interface.
use gtk::{cairo, gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::FRAC_PI_2;

pub(crate) const KEEP_ROUND: &str = "capy-keep-round";
const CORNER_SEGMENTS: u32 = 24;
const MASK_PRUNE_THRESHOLD: usize = 512;
pub const CORNER_FIT: f32 = 0.54;
const SHADOW_SLICES: [(usize, usize); 8] = [(0, 0), (1, 0), (2, 0), (0, 1), (2, 1), (0, 2), (1, 2), (2, 2)];

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

struct ShadowGeometry {
    key: [u32; 22],
    insets: [f32; 4],
    edge: usize,
    inset: usize,
}

impl ShadowGeometry {
    fn new(scale: f64, shape: &gsk::RoundedRect, outline: &gsk::RoundedRect, blur: f32) -> Self {
        let (s, o) = (shape.bounds(), outline.bounds());
        let quantum = 16. * scale as f32;
        let insets = [
            o.x() - s.x(),
            o.y() - s.y(),
            s.x() + s.width() - o.x() - o.width(),
            s.y() + s.height() - o.y() - o.height(),
        ]
        .map(|inset| (inset * quantum).round() / quantum);
        let corners = shape.corner().iter().chain(outline.corner()).flat_map(|c| [c.width(), c.height()]);
        let reach = corners.clone().fold(0f32, f32::max) + insets.iter().fold(0f32, |m, v| m.max(v.abs()));
        let edge = (1.5 * f64::from(blur) * scale).ceil() as usize;
        let inset = (f64::from(reach) * scale).ceil() as usize + edge + 1;
        let mut key = [0; 22];
        let values = [scale as f32, blur].into_iter().chain(corners).chain(insets);
        for (slot, value) in key.iter_mut().zip(values) {
            *slot = value.to_bits();
        }
        Self { key, insets, edge, inset }
    }

    fn slices(&self, scale: f64, shape: &gsk::RoundedRect, outline: &gsk::RoundedRect, blur: f32) -> [gdk::Texture; 8] {
        let size = 2 * (self.edge + self.inset) + 1;
        let span = (2 * self.inset + 1) as f32 / scale as f32;
        let [i_left, i_top, i_right, i_bottom] = self.insets;
        let [tl, tr, br, bl] = *shape.corner();
        let shape = gsk::RoundedRect::new(graphene::Rect::new(0., 0., span, span), tl, tr, br, bl);
        let [tl, tr, br, bl] = *outline.corner();
        let outline = gsk::RoundedRect::new(
            graphene::Rect::new(i_left, i_top, span - i_left - i_right, span - i_top - i_bottom),
            tl,
            tr,
            br,
            bl,
        );
        let origin = self.edge as f64 / scale;
        let mut alpha = coverage(size, scale, origin, &shape);
        gaussian_blur(&mut alpha, size, 0.5 * f64::from(blur) * scale);
        for (value, covered) in alpha.iter_mut().zip(coverage(size, scale, origin, &outline)) {
            *value *= 1. - covered;
        }
        let cut = [0, self.edge + self.inset, self.edge + self.inset + 1, size];
        SHADOW_SLICES.map(|(column, row)| {
            let [x0, x1, y0, y1] = [cut[column], cut[column + 1], cut[row], cut[row + 1]];
            let bytes: Vec<u8> = (y0..y1)
                .flat_map(|y| alpha[y * size + x0..y * size + x1].iter().map(|v| (v * 255.).round().clamp(0., 255.) as u8))
                .collect();
            gdk::MemoryTexture::new(
                (x1 - x0) as i32,
                (y1 - y0) as i32,
                gdk::MemoryFormat::A8,
                &glib::Bytes::from_owned(bytes),
                x1 - x0,
            )
            .upcast()
        })
    }
}

fn coverage(size: usize, scale: f64, origin: f64, rect: &gsk::RoundedRect) -> Vec<f32> {
    let mut surface = cairo::ImageSurface::create(cairo::Format::A8, size as i32, size as i32)
        .expect("allocate shadow shape");
    {
        let cr = cairo::Context::new(&surface).expect("draw shadow shape");
        cr.scale(scale, scale);
        cr.translate(origin, origin);
        rounded_rect(&cr, rect);
        let _ = cr.fill();
    }
    let stride = surface.stride() as usize;
    let data = surface.data().expect("read shadow shape");
    (0..size).flat_map(|y| data[y * stride..][..size].iter().map(|&a| f32::from(a) / 255.)).collect()
}

fn box_widths(sigma: f64) -> [usize; 3] {
    let ideal = (4. * sigma * sigma + 1.).sqrt().floor().max(1.) as usize;
    let lower = if ideal.is_multiple_of(2) { ideal - 1 } else { ideal };
    let l = lower as f64;
    let lower_count = ((12. * sigma * sigma - 3. * l * l - 12. * l - 9.) / (-4. * l - 4.)).round();
    std::array::from_fn(|i| if (i as f64) < lower_count { lower } else { lower + 2 })
}

fn gaussian_blur(values: &mut [f32], size: usize, sigma: f64) {
    let mut line = vec![0f32; size];
    for width in box_widths(sigma) {
        let radius = width / 2;
        for (step, stride) in [(1, size), (size, 1)] {
            for start in (0..size).map(|i| i * stride) {
                for (i, value) in line.iter_mut().enumerate() {
                    *value = values[start + i * step];
                }
                let mut sum: f32 = line[..=radius.min(size - 1)].iter().sum();
                for i in 0..size {
                    values[start + i * step] = sum / width as f32;
                    if i + radius + 1 < size {
                        sum += line[i + radius + 1];
                    }
                    if i >= radius {
                        sum -= line[i - radius];
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct Converter {
    scale: f64,
    nodes: HashMap<usize, (gsk::RenderNode, gsk::RenderNode)>,
    previous: HashMap<usize, (gsk::RenderNode, gsk::RenderNode)>,
    masks: HashMap<MaskKey, glib::WeakRef<gdk::Texture>>,
    shadows: HashMap<[u32; 22], [gdk::Texture; 8]>,
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
        if self.shadows.len() > MASK_PRUNE_THRESHOLD {
            self.shadows.clear();
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
            if outline.is_rectilinear() {
                return node.clone();
            }
            let spread = shadow.spread();
            let inner = squircle_rect(&outline);
            let mut outer = inner;
            outer.offset(shadow.dx(), shadow.dy());
            outer.shrink(-spread, -spread, -spread, -spread);
            let blur = shadow.blur_radius();
            if blur > 0. {
                return self.blurred_shadow(&outer, &inner, &shadow.color(), blur);
            }
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

    fn blurred_shadow(
        &mut self,
        shape: &gsk::RoundedRect,
        outline: &gsk::RoundedRect,
        color: &gdk::RGBA,
        blur: f32,
    ) -> gsk::RenderNode {
        let geometry = ShadowGeometry::new(self.scale, shape, outline, blur);
        let bounds = shape.bounds();
        let scale = self.scale as f32;
        let minimum = (2 * geometry.inset + 1) as f32 / scale;
        if bounds.width() < minimum || bounds.height() < minimum {
            let fill = gsk::ColorNode::new(color, bounds);
            let shadow = gsk::MaskNode::new(fill, self.mask(shape, None), gsk::MaskMode::Alpha);
            let blurred = gsk::BlurNode::new(shadow, blur);
            return gsk::MaskNode::new(blurred, self.mask(outline, None), gsk::MaskMode::InvertedAlpha).upcast();
        }
        let textures = self
            .shadows
            .entry(geometry.key)
            .or_insert_with(|| geometry.slices(self.scale, shape, outline, blur));
        let (edge, inset) = (geometry.edge as f32 / scale, geometry.inset as f32 / scale);
        let (x, y) = (bounds.x(), bounds.y());
        let (right, bottom) = (x + bounds.width(), y + bounds.height());
        let xs = [x - edge, x + inset, right - inset, right + edge];
        let ys = [y - edge, y + inset, bottom - inset, bottom + edge];
        let slices: Vec<_> = SHADOW_SLICES
            .into_iter()
            .zip(textures.iter())
            .map(|((column, row), texture)| {
                let area = graphene::Rect::new(xs[column], ys[row], xs[column + 1] - xs[column], ys[row + 1] - ys[row]);
                gsk::MaskNode::new(
                    gsk::ColorNode::new(color, &area),
                    gsk::TextureNode::new(texture, &area),
                    gsk::MaskMode::Alpha,
                )
                .upcast()
            })
            .collect();
        gsk::ContainerNode::new(&slices).upcast()
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
