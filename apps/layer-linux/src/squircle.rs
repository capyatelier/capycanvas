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

pub(crate) fn corner(cr: &cairo::Context, center: [f64; 2], start: [f64; 2], end: [f64; 2]) {
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

pub(crate) fn path(draw: impl FnOnce(&cairo::Context)) -> gsk::Path {
    let surface = cairo::RecordingSurface::create(cairo::Content::Alpha, None).unwrap();
    let cr = cairo::Context::new(&surface).unwrap();
    draw(&cr);
    let builder = gsk::PathBuilder::new();
    builder.add_cairo_path(&cr.copy_path().unwrap());
    builder.to_path()
}

fn grid_transform(transform: &gsk::Transform) -> bool {
    if !matches!(transform.category(), gsk::TransformCategory::Identity | gsk::TransformCategory::_2dTranslate
        | gsk::TransformCategory::_2dAffine | gsk::TransformCategory::_2d) { return false; }
    let (xx, yx, xy, yy, dx, dy) = transform.to_2d();
    [xx, yx, xy, yy, dx, dy].into_iter().all(|value| value.fract() == 0.)
        && xx.abs() + yx.abs() == 1. && xy.abs() + yy.abs() == 1. && xx * xy + yx * yy == 0.
}

fn grid_bounds(bounds: &graphene::Rect, scale: f64) -> bool {
    [bounds.x(), bounds.y(), bounds.width(), bounds.height()].into_iter()
        .all(|value| (f64::from(value) * scale).fract() == 0.)
}

#[derive(Clone, Copy)]
pub(crate) struct PixelGrid {
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

impl PixelGrid {
    fn new(scale: f64) -> Self { Self { scale: [scale as f32; 2], offset: [0.; 2] } }

    pub fn for_widget(widget: &gtk::Widget) -> Self {
        let scale = widget.native().and_then(|native| native.surface())
            .map_or(widget.scale_factor() as f64, |surface| surface.scale());
        let grid = Self::new(scale);
        widget.root().and_then(|root| root.dynamic_cast::<gtk::Widget>().ok())
            .and_then(|root| widget.compute_transform(&root))
            .map_or(grid, |matrix| grid.transform(&gsk::Transform::new().matrix(&matrix)))
    }

    fn transform(self, transform: &gsk::Transform) -> Self {
        if !matches!(transform.category(), gsk::TransformCategory::Identity | gsk::TransformCategory::_2dTranslate
            | gsk::TransformCategory::_2dAffine | gsk::TransformCategory::_2d) {
            return Self { scale: [0.; 2], offset: [0.; 2] };
        }
        let (xx, yx, xy, yy, dx, dy) = transform.to_2d();
        if xy == 0. && yx == 0. && xx != 0. && yy != 0. {
            Self { scale: [self.scale[0] * xx, self.scale[1] * yy],
                offset: [self.offset[0] + self.scale[0] * dx, self.offset[1] + self.scale[1] * dy] }
        } else if xx == 0. && yy == 0. && xy != 0. && yx != 0. {
            Self { scale: [self.scale[1] * yx, self.scale[0] * xy],
                offset: [self.offset[1] + self.scale[1] * dy, self.offset[0] + self.scale[0] * dx] }
        } else { Self { scale: [0.; 2], offset: [0.; 2] } }
    }

    pub fn floor(self, axis: usize, value: f32) -> f32 {
        if self.scale[axis] == 0. { return value; }
        let pixel = value * self.scale[axis] + self.offset[axis];
        let edge = if self.scale[axis] > 0. { pixel.floor() } else { pixel.ceil() };
        (edge - self.offset[axis]) / self.scale[axis]
    }

    pub fn ceil(self, axis: usize, value: f32) -> f32 {
        if self.scale[axis] == 0. { return value; }
        let pixel = value * self.scale[axis] + self.offset[axis];
        let edge = if self.scale[axis] > 0. { pixel.ceil() } else { pixel.floor() };
        (edge - self.offset[axis]) / self.scale[axis]
    }

    fn key(self) -> [u32; 4] {
        [self.scale[0], self.scale[1], self.offset[0].rem_euclid(1.), self.offset[1].rem_euclid(1.)].map(f32::to_bits)
    }
}

impl Default for PixelGrid { fn default() -> Self { Self::new(1.) } }

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
        let mut alpha = coverage([size; 2], scale, origin, &shape);
        gaussian_blur(&mut alpha, [size; 2], 0.5 * f64::from(blur) * scale);
        for (value, covered) in alpha.iter_mut().zip(coverage([size; 2], scale, origin, &outline)) {
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

fn coverage([width, height]: [usize; 2], scale: f64, origin: f64, rect: &gsk::RoundedRect) -> Vec<f32> {
    let mut surface = cairo::ImageSurface::create(cairo::Format::A8, width as i32, height as i32)
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
    (0..height).flat_map(|y| data[y * stride..][..width].iter().map(|&a| f32::from(a) / 255.)).collect()
}

fn box_widths(sigma: f64) -> [usize; 3] {
    let ideal = (4. * sigma * sigma + 1.).sqrt().floor().max(1.) as usize;
    let lower = if ideal.is_multiple_of(2) { ideal - 1 } else { ideal };
    let l = lower as f64;
    let lower_count = ((12. * sigma * sigma - 3. * l * l - 12. * l - 9.) / (-4. * l - 4.)).round();
    std::array::from_fn(|i| if (i as f64) < lower_count { lower } else { lower + 2 })
}

fn gaussian_blur(values: &mut [f32], [width, height]: [usize; 2], sigma: f64) {
    let mut line = vec![0f32; width.max(height)];
    for box_width in box_widths(sigma) {
        let radius = box_width / 2;
        for (step, stride, length, count) in [(1, width, width, height), (width, 1, height, width)] {
            for start in (0..count).map(|i| i * stride) {
                for (i, value) in line[..length].iter_mut().enumerate() {
                    *value = values[start + i * step];
                }
                let mut sum: f32 = line[..=radius.min(length - 1)].iter().sum();
                for i in 0..length {
                    values[start + i * step] = sum / box_width as f32;
                    if i + radius + 1 < length {
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
    aligned: bool,
    grid: PixelGrid,
    window: Option<[f32; 3]>,
    nodes: HashMap<(usize, bool, [u32; 4], [u32; 9]), (gsk::RenderNode, gsk::RenderNode)>,
    previous: HashMap<(usize, bool, [u32; 4], [u32; 9]), (gsk::RenderNode, gsk::RenderNode)>,
    masks: HashMap<MaskKey, glib::WeakRef<gdk::Texture>>,
    shadows: HashMap<[u32; 22], [gdk::Texture; 8]>,
    corners: HashMap<[u32; 5], gsk::Path>,
    rings: HashMap<(MaskKey, [u32; 2]), gsk::Path>,
    compact_shadows: HashMap<[u32; 31], [gdk::Texture; 3]>,
}

impl Converter {
    fn frame(&mut self, node: &gsk::RenderNode, scale: f64) -> gsk::RenderNode {
        self.frame_in_window(node, scale, None)
    }

    fn frame_in_window(&mut self, node: &gsk::RenderNode, scale: f64, window: Option<[f32; 3]>) -> gsk::RenderNode {
        if self.scale != scale {
            *self = Self { scale, ..Self::default() };
        }
        self.window = window;
        self.aligned = scale.fract() == 0.;
        self.grid = PixelGrid::new(scale);
        self.previous = std::mem::take(&mut self.nodes);
        if self.masks.len() > MASK_PRUNE_THRESHOLD {
            self.masks.retain(|_, mask| mask.upgrade().is_some());
        }
        if self.shadows.len() > MASK_PRUNE_THRESHOLD {
            self.shadows.clear();
        }
        if self.corners.len() > MASK_PRUNE_THRESHOLD { self.corners.clear(); }
        if self.rings.len() > MASK_PRUNE_THRESHOLD { self.rings.clear(); }
        if self.compact_shadows.len() > MASK_PRUNE_THRESHOLD { self.compact_shadows.clear(); }
        let converted = self.convert(node);
        self.previous.clear();
        converted
    }

    fn convert(&mut self, node: &gsk::RenderNode) -> gsk::RenderNode {
        let key = (node.as_ptr() as usize, self.aligned, self.grid.key(), self.window_key(&node.bounds()));
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

    fn window_key(&self, bounds: &graphene::Rect) -> [u32; 9] {
        let mut key = [0; 9];
        let Some([width, height, radius]) = self.window else { return key };
        if self.grid.scale != [self.scale as f32; 2] { return key; }
        let position = self.grid.offset.map(|offset| offset / self.scale as f32);
        let world = graphene::Rect::new(bounds.x() + position[0], bounds.y() + position[1], bounds.width(), bounds.height());
        for (i, [x, y]) in [[0., 0.], [width - radius, 0.], [width - radius, height - radius], [0., height - radius]].into_iter().enumerate() {
            if world.intersection(&graphene::Rect::new(x, y, radius, radius)).is_some() {
                key[i * 2] = (x - position[0]).to_bits();
                key[i * 2 + 1] = (y - position[1]).to_bits();
                key[8] = radius.to_bits();
            }
        }
        key
    }

    fn content_clip(&self, shape: &gsk::RoundedRect) -> gsk::RoundedRect {
        let mut radii = shape.corner().map(|radius|
            graphene::Size::new(radius.width() * CORNER_FIT, radius.height() * CORNER_FIT));
        if let Some([width, height, radius]) = self.window
            && self.grid.scale == [self.scale as f32; 2] {
            let [x, y] = self.grid.offset.map(|offset| offset / self.scale as f32);
            let bounds = shape.bounds();
            let left = bounds.x() + x;
            let top = bounds.y() + y;
            let right = width - left - bounds.width();
            let bottom = height - top - bounds.height();
            if [left, top, right, bottom].into_iter().all(|margin| margin >= 0.) {
                for (corner, [dx, dy]) in radii.iter_mut().zip([[left, top], [right, top], [right, bottom], [left, bottom]]) {
                    if dx <= radius && dy <= radius {
                        *corner = graphene::Size::new(corner.width().max(radius - dx), corner.height().max(radius - dy));
                    }
                }
            }
        }
        gsk::RoundedRect::new(*shape.bounds(), radii[0], radii[1], radii[2], radii[3])
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

    fn rebuild_pair(
        &mut self,
        node: &gsk::RenderNode,
        first: &gsk::RenderNode,
        second: &gsk::RenderNode,
        wrap: impl FnOnce(&gsk::RenderNode, &gsk::RenderNode) -> gsk::RenderNode,
    ) -> gsk::RenderNode {
        let a = self.convert(first);
        let b = self.convert(second);
        if a.as_ptr() == first.as_ptr() && b.as_ptr() == second.as_ptr() {
            node.clone()
        } else { wrap(&a, &b) }
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
            let aligned = self.aligned;
            let grid = self.grid;
            self.grid = grid.transform(&matrix);
            self.aligned &= grid_transform(&matrix);
            let converted = self.rebuild(node, &transform.child(), |child| {
                gsk::TransformNode::new(child, Some(&matrix)).upcast()
            });
            self.aligned = aligned;
            self.grid = grid;
            return converted;
        }
        if let Some(clip) = node.downcast_ref::<gsk::ClipNode>() {
            let rect = clip.clip();
            let aligned = self.aligned;
            self.aligned &= grid_bounds(&rect, self.scale);
            let converted = self.rebuild(node, &clip.child(), |child| gsk::ClipNode::new(child, &rect).upcast());
            self.aligned = aligned;
            return converted;
        }
        if let Some(opacity) = node.downcast_ref::<gsk::OpacityNode>() {
            let alpha = opacity.opacity();
            return self.rebuild(node, &opacity.child(), |child| gsk::OpacityNode::new(child, alpha).upcast());
        }
        if let Some(mask) = node.downcast_ref::<gsk::MaskNode>() {
            let aligned = self.aligned;
            self.aligned &= grid_bounds(&node.bounds(), self.scale);
            let converted = self.rebuild_pair(node, &mask.source(), &mask.mask(), |source, child| {
                gsk::MaskNode::new(source, child, mask.mask_mode()).upcast()
            });
            self.aligned = aligned;
            return converted;
        }
        if let Some(color) = node.downcast_ref::<gsk::ColorMatrixNode>() {
            return self.rebuild(node, &color.child(), |child| {
                gsk::ColorMatrixNode::new(child, &color.color_matrix(), &color.color_offset()).upcast()
            });
        }
        if let Some(blur) = node.downcast_ref::<gsk::BlurNode>() {
            return self.rebuild(node, &blur.child(), |child| gsk::BlurNode::new(child, blur.radius()).upcast());
        }
        if let Some(fade) = node.downcast_ref::<gsk::CrossFadeNode>() {
            return self.rebuild_pair(node, &fade.start_child(), &fade.end_child(), |start, end| {
                gsk::CrossFadeNode::new(start, end, fade.progress()).upcast()
            });
        }
        if let Some(blend) = node.downcast_ref::<gsk::BlendNode>() {
            return self.rebuild_pair(node, &blend.bottom_child(), &blend.top_child(), |bottom, top| {
                gsk::BlendNode::new(bottom, top, blend.blend_mode()).upcast()
            });
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
            let aligned = self.aligned;
            self.aligned &= grid_bounds(clip.bounds(), self.scale);
            let converted = if clip.is_rectilinear() {
                self.rebuild(node, &rounded.child(), |child| gsk::RoundedClipNode::new(child, &clip).upcast())
            } else {
                let child = self.convert(&rounded.child());
                self.rounded(&child, &squircle_rect(&clip))
            };
            self.aligned = aligned;
            return converted;
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

    fn rounded_parts(&self, shape: &gsk::RoundedRect) -> Option<[f32; 4]> {
        let bounds = shape.bounds();
        if self.grid.scale.into_iter().any(|scale| scale == 0.)
            || f64::from(bounds.width().max(bounds.height())) * self.scale <= 128. { return None; }
        let [tl, tr, br, bl] = *shape.corner();
        let left = self.grid.ceil(0, bounds.x() + tl.width().max(bl.width())) - bounds.x();
        let right = bounds.x() + bounds.width() - self.grid.floor(0, bounds.x() + bounds.width() - tr.width().max(br.width()));
        let top = self.grid.ceil(1, bounds.y() + tl.height().max(tr.height())) - bounds.y();
        let bottom = bounds.y() + bounds.height() - self.grid.floor(1, bounds.y() + bounds.height() - bl.height().max(br.height()));
        if left + right > bounds.width() || top + bottom > bounds.height() { None }
        else { Some([left, right, top, bottom]) }
    }

    fn rounded(&mut self, child: &gsk::RenderNode, shape: &gsk::RoundedRect) -> gsk::RenderNode {
        if !child.is::<gsk::ColorNode>() {
            if f64::from(shape.bounds().width().max(shape.bounds().height())) * self.scale <= 128. {
                return gsk::MaskNode::new(child, self.mask(shape, None), gsk::MaskMode::Alpha).upcast();
            }
            let clip = self.content_clip(shape);
            return gsk::RoundedClipNode::new(child, &clip).upcast();
        }
        let bounds = shape.bounds();
        let Some([left, right, top, bottom]) = self.rounded_parts(shape) else {
            let mut local = *shape;
            local.offset(-bounds.x(), -bounds.y());
            let key = (MaskKey::new(self.scale, &local, None), [0; 2]);
            let path = self.rings.entry(key).or_insert_with(|| path(|cr| rounded_rect(cr, &local)));
            let local = if child.bounds().contains_rect(bounds) && child.bounds().contains_rect(local.bounds()) {
                child.clone()
            } else {
                gsk::TransformNode::new(child, Some(&gsk::Transform::new().translate(
                    &graphene::Point::new(-bounds.x(), -bounds.y())))).upcast()
            };
            let fill = gsk::FillNode::new(local, path, gsk::FillRule::Winding);
            return gsk::TransformNode::new(fill, Some(&gsk::Transform::new().translate(
                &graphene::Point::new(bounds.x(), bounds.y())))).upcast();
        };
        let [tl, tr, br, bl] = *shape.corner();
        let [x, y, width, height] = [bounds.x(), bounds.y(), bounds.width(), bounds.height()];
        let mut pieces = Vec::with_capacity(7);
        let mut rectangle = |rect: graphene::Rect| {
            if let Some(rect) = rect.intersection(&child.bounds()) {
                pieces.push(gsk::ClipNode::new(child, &rect).upcast());
            }
        };
        rectangle(graphene::Rect::new(x, y + top, width, height - top - bottom));
        rectangle(graphene::Rect::new(x + left, y, width - left - right, top));
        rectangle(graphene::Rect::new(x + left, y + height - bottom, width - left - right, bottom));
        for (index, origin, size, radius) in [
            (0, [x, y], [left, top], tl),
            (1, [x + width - right, y], [right, top], tr),
            (2, [x + width - right, y + height - bottom], [right, bottom], br),
            (3, [x, y + height - bottom], [left, bottom], bl),
        ] {
            if size[0] <= 0. || size[1] <= 0. { continue; }
            let key = [size[0].to_bits(), size[1].to_bits(), radius.width().to_bits(),
                radius.height().to_bits(), index as u32];
            let path = self.corners.entry(key).or_insert_with(|| {
                let mut radii = [graphene::Size::new(0., 0.); 4];
                radii[index] = radius;
                let corner = gsk::RoundedRect::new(graphene::Rect::new(0., 0., size[0], size[1]),
                    radii[0], radii[1], radii[2], radii[3]);
                path(|cr| rounded_rect(cr, &corner))
            });
            let local = if child.is::<gsk::ColorNode>()
                && child.bounds().contains_rect(&graphene::Rect::new(origin[0], origin[1], size[0], size[1]))
                && child.bounds().contains_rect(&graphene::Rect::new(0., 0., size[0], size[1])) {
                child.clone()
            } else {
                gsk::TransformNode::new(child, Some(&gsk::Transform::new()
                    .translate(&graphene::Point::new(-origin[0], -origin[1])))).upcast()
            };
            let fill = gsk::FillNode::new(local, path, gsk::FillRule::Winding);
            let tile = gsk::ClipNode::new(fill, &graphene::Rect::new(0., 0., size[0], size[1]));
            pieces.push(gsk::TransformNode::new(tile, Some(&gsk::Transform::new()
                .translate(&graphene::Point::new(origin[0], origin[1])))).upcast());
        }
        gsk::ContainerNode::new(&pieces).upcast()
    }

    fn ring(&mut self, outer: &gsk::RoundedRect, inner: &gsk::RoundedRect, color: &gdk::RGBA) -> gsk::RenderNode {
        let bounds = outer.bounds();
        let inside = inner.bounds();
        let [tl, tr, br, bl] = *outer.corner();
        let [itl, itr, ibr, ibl] = *inner.corner();
        let right = bounds.x() + bounds.width();
        let bottom = bounds.y() + bounds.height();
        let split = |axis, start, length, near: f32, far: f32| {
            let first = self.grid.ceil(axis, near).clamp(start, start + length);
            let last = self.grid.floor(axis, far).clamp(start, start + length);
            if first >= last { [start, start + length, start + length, start + length] }
            else { [start, first, last, start + length] }
        };
        let xs = split(0, bounds.x(), bounds.width(),
            (bounds.x() + tl.width().max(bl.width())).max(inside.x() + itl.width().max(ibl.width())),
            (right - tr.width().max(br.width())).min(inside.x() + inside.width() - itr.width().max(ibr.width())));
        let ys = split(1, bounds.y(), bounds.height(),
            (bounds.y() + tl.height().max(tr.height())).max(inside.y() + itl.height().max(itr.height())),
            (bottom - bl.height().max(br.height())).min(inside.y() + inside.height() - ibl.height().max(ibr.height())));
        let pad = outer.corner().iter().chain(inner.corner()).map(|r| r.width().max(r.height())).fold(1., f32::max) + 1.;
        let mut pieces = Vec::with_capacity(8);
        for row in 0..3 { for column in 0..3 {
            let area = graphene::Rect::new(xs[column], ys[row], xs[column + 1] - xs[column], ys[row + 1] - ys[row]);
            if area.width() <= 0. || area.height() <= 0. { continue; }
            let left = xs[column] == bounds.x();
            let top = ys[row] == bounds.y();
            let right_edge = xs[column + 1] == right;
            let bottom_edge = ys[row + 1] == bottom;
            if !(left || right_edge) || !(top || bottom_edge) {
                let mut append = |rect: graphene::Rect| {
                    if rect.width() > 0. && rect.height() > 0. { pieces.push(gsk::ColorNode::new(color, &rect).upcast()); }
                };
                if let Some(cut) = area.intersection(inside) {
                    append(graphene::Rect::new(area.x(), area.y(), area.width(), cut.y() - area.y()));
                    append(graphene::Rect::new(area.x(), cut.y() + cut.height(), area.width(), area.y() + area.height() - cut.y() - cut.height()));
                    append(graphene::Rect::new(area.x(), cut.y(), cut.x() - area.x(), cut.height()));
                    append(graphene::Rect::new(cut.x() + cut.width(), cut.y(), area.x() + area.width() - cut.x() - cut.width(), cut.height()));
                } else { append(area); }
                continue;
            }
            let local = |shape: &gsk::RoundedRect| {
                let b = shape.bounds();
                let x = if left { b.x() - area.x() } else { -pad };
                let y = if top { b.y() - area.y() } else { -pad };
                let x1 = if right_edge { b.x() + b.width() - area.x() } else { area.width() + pad };
                let y1 = if bottom_edge { b.y() + b.height() - area.y() } else { area.height() + pad };
                let square = graphene::Size::new(0., 0.);
                let radii = shape.corner();
                gsk::RoundedRect::new(graphene::Rect::new(x, y, x1 - x, y1 - y),
                    if left && top { radii[0] } else { square },
                    if right_edge && top { radii[1] } else { square },
                    if right_edge && bottom_edge { radii[2] } else { square },
                    if left && bottom_edge { radii[3] } else { square })
            };
            let outer = local(outer);
            let inner = local(inner);
            let key = (MaskKey::new(self.scale, &outer, Some(&inner)), [outer.bounds().x(), outer.bounds().y()].map(f32::to_bits));
            let path = self.rings.entry(key).or_insert_with(|| path(|cr| {
                rounded_rect(cr, &outer);
                rounded_rect(cr, &inner);
            }));
            let fill = gsk::FillNode::new(gsk::ColorNode::new(color, &graphene::Rect::new(0., 0., area.width(), area.height())),
                path, gsk::FillRule::EvenOdd);
            pieces.push(gsk::TransformNode::new(fill, Some(&gsk::Transform::new().translate(
                &graphene::Point::new(area.x(), area.y())))).upcast());
        }}
        gsk::ContainerNode::new(&pieces).upcast()
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
            return self.compact_shadow(shape, outline, color, blur, &geometry);
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

    fn compact_shadow(&mut self, shape: &gsk::RoundedRect, outline: &gsk::RoundedRect,
        color: &gdk::RGBA, blur: f32, geometry: &ShadowGeometry) -> gsk::RenderNode {
        let bounds = shape.bounds();
        let scale = self.scale as f32;
        let vertical = bounds.width() < bounds.height();
        let [short, length] = if vertical { [bounds.width(), bounds.height()] }
            else { [bounds.height(), bounds.width()] };
        let minimum = (2 * geometry.inset + 1) as f32 / scale;
        let reduction = ((length - minimum).max(0.) * scale).floor() / scale;
        let minimum = length - reduction;
        let original = outline.bounds();
        let inner_size = if vertical { [original.width(), original.height() - reduction] }
            else { [original.width() - reduction, original.height()] };
        let insets = [original.x() - bounds.x(), original.y() - bounds.y(),
            bounds.x() + bounds.width() - original.x() - original.width(),
            bounds.y() + bounds.height() - original.y() - original.height()];
        let mut key = [0; 31];
        key[..22].copy_from_slice(&geometry.key);
        key[18..22].copy_from_slice(&insets.map(f32::to_bits));
        key[22] = u32::from(vertical);
        key[23] = short.to_bits();
        key[28] = minimum.to_bits();
        key[29..31].copy_from_slice(&inner_size.map(f32::to_bits));
        let [width, height] = if vertical { [short, minimum] } else { [minimum, short] };
        let pixels = [width, height].map(|value| (value * scale).ceil().max(1.) as usize + 2 * geometry.edge);
        let axis = usize::from(vertical);
        let cut = (geometry.edge + geometry.inset).min((pixels[axis] - 1) / 2);
        let textures = self.compact_shadows.entry(key).or_insert_with(|| {
            let make = |bounds, corners: &[graphene::Size; 4]| gsk::RoundedRect::new(bounds,
                corners[0], corners[1], corners[2], corners[3]);
            let outer = make(graphene::Rect::new(0., 0., width, height), shape.corner());
            let inner = make(graphene::Rect::new(insets[0], insets[1],
                inner_size[0], inner_size[1]), outline.corner());
            let origin = geometry.edge as f64 / self.scale;
            let mut alpha = coverage(pixels, self.scale, origin, &outer);
            gaussian_blur(&mut alpha, pixels, 0.5 * f64::from(blur) * self.scale);
            for (value, covered) in alpha.iter_mut().zip(coverage(pixels, self.scale, origin, &inner)) {
                *value *= 1. - covered;
            }
            std::array::from_fn(|index| {
                let [start, end] = match index { 0 => [0, cut + 1], 1 => [cut, cut + 1], _ => [cut, pixels[axis]] };
                let [x0, x1, y0, y1] = if vertical { [0, pixels[0], start, end] }
                    else { [start, end, 0, pixels[1]] };
                let bytes: Vec<u8> = (y0..y1).flat_map(|y| alpha[y * pixels[0] + x0..y * pixels[0] + x1]
                    .iter().map(|value| (value * 255.).round().clamp(0., 255.) as u8)).collect();
                gdk::MemoryTexture::new((x1 - x0) as i32, (y1 - y0) as i32, gdk::MemoryFormat::A8,
                    &glib::Bytes::from_owned(bytes), x1 - x0).upcast()
            })
        });
        let edge = geometry.edge as f32 / scale;
        let first = cut as f32 / scale;
        let pixel = 1. / scale;
        let origin = if vertical { bounds.y() } else { bounds.x() };
        let first_end = self.grid.ceil(axis, origin + first - edge) - origin;
        let last_start = self.grid.floor(axis, origin + first + pixel + reduction - edge) - origin;
        let lengths = [first + pixel, last_start - first_end, (pixels[axis] - cut) as f32 / scale];
        let starts = [-edge, first_end, first + reduction - edge];
        let pieces: Vec<_> = textures.iter().enumerate().filter_map(|(index, texture)| {
            if lengths[index] <= 0. { return None; }
            let rect = if vertical {
                graphene::Rect::new(bounds.x() - edge, bounds.y() + starts[index], pixels[0] as f32 / scale, lengths[index])
            } else {
                graphene::Rect::new(bounds.x() + starts[index], bounds.y() - edge, lengths[index], pixels[1] as f32 / scale)
            };
            let node = gsk::MaskNode::new(gsk::ColorNode::new(color, &rect), gsk::TextureNode::new(texture, &rect),
                gsk::MaskMode::Alpha).upcast();
            let [start, end] = match index { 0 => [-edge, first_end], 2 => [last_start, starts[index] + lengths[index]],
                _ => return Some(node) };
            let clip = if vertical {
                graphene::Rect::new(rect.x(), bounds.y() + start, rect.width(), end - start)
            } else { graphene::Rect::new(bounds.x() + start, rect.y(), end - start, rect.height()) };
            Some(gsk::ClipNode::new(&node, &clip).upcast())
        }).collect();
        gsk::ContainerNode::new(&pieces).upcast()
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

pub(crate) fn converted(node: &gsk::RenderNode, scale: f64) -> gsk::RenderNode {
    Converter::default().frame(node, scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(radius: f32) -> gsk::RenderNode {
        let bounds = graphene::Rect::new(0., 0., 80., 120.);
        let outline = gsk::RoundedRect::new(
            bounds,
            graphene::Size::new(18., 18.),
            graphene::Size::new(18., 18.),
            graphene::Size::new(radius, radius),
            graphene::Size::new(radius, radius),
        );
        gsk::ContainerNode::new(&[
            gsk::RoundedClipNode::new(gsk::ColorNode::new(&gdk::RGBA::WHITE, &bounds), &outline).upcast(),
            gsk::OutsetShadowNode::new(&outline, &gdk::RGBA::BLACK, 0., 2., 0., 6.).upcast(),
        ]).upcast()
    }

    fn assert_converted_panel(node: &gsk::RenderNode) {
        let panel = node.downcast_ref::<gsk::ContainerNode>().unwrap();
        assert!(matches!(panel.child(0).node_type(), gsk::RenderNodeType::ContainerNode | gsk::RenderNodeType::TransformNode | gsk::RenderNodeType::FillNode), "solid panel keeps its squircle fill");
        assert_eq!(panel.child(0).bounds(), graphene::Rect::new(0., 0., 80., 120.));
        assert!(!panel.child(1).is::<gsk::OutsetShadowNode>(), "native shadow must be converted inside wrappers");
    }

    #[test]
    fn panel_resize_reuses_fixed_corner_paths_without_full_panel_masks() {
        for scale in [1., 2.] {
            let mut converter = Converter::default();
            let mut first = None;
            for height in [170., 220., 320., 170.] {
                let bounds = graphene::Rect::new(0., 0., 242., height);
                let node = gsk::RoundedClipNode::new(gsk::ColorNode::new(&gdk::RGBA::WHITE, &bounds),
                    &gsk::RoundedRect::from_rect(bounds, 9.72));
                let result = converter.frame(node.upcast_ref(), scale);
                assert_eq!(result.bounds(), bounds);
                assert!(converter.masks.is_empty());
                assert_eq!(converter.corners.len(), 4);
                let mut paths: Vec<_> = converter.corners.iter().map(|(key, path)| {
                    let bounds = path.bounds().unwrap();
                    assert!(f64::from(bounds.width().max(bounds.height())) * scale <= 256.);
                    (*key, path.as_ptr() as usize)
                }).collect();
                paths.sort_unstable();
                if let Some(first) = &first { assert_eq!(&paths, first); } else { first = Some(paths); }
            }
        }
    }

    #[test]
    fn fractional_panel_groups_do_not_add_masks() {
        fn check(node: &gsk::RenderNode) {
            assert!(!node.is::<gsk::MaskNode>(), "fractional groups retain native fills");
            for child in node.children() { check(&child); }
        }
        for scale in [1., 1.25, 2.] {
            let bounds = graphene::Rect::new(0.25, 0.5, 242., 170.);
            let glyph_bounds = graphene::Rect::new(8.25, 8.5, 16., 14.);
            let glyph = gsk::ContainerNode::new(&[
                gsk::ColorNode::new(&gdk::RGBA::WHITE, &glyph_bounds).upcast(),
                gsk::ColorNode::new(&gdk::RGBA::BLACK, &glyph_bounds).upcast(),
            ]);
            let content = gsk::ContainerNode::new(&[
                gsk::ColorNode::new(&gdk::RGBA::WHITE, &bounds).upcast(), glyph.upcast(),
            ]);
            let node = gsk::RoundedClipNode::new(content, &gsk::RoundedRect::from_rect(bounds, 9.72));
            let mut converter = Converter::default();
            let converted = converter.frame(node.upcast_ref(), scale);
            check(&converted);
            assert!(converter.masks.is_empty());
            assert_eq!(converter.frame(node.upcast_ref(), scale).as_ptr(), converted.as_ptr());
        }
    }

    #[test]
    fn long_toolbar_shadows_reuse_their_fixed_source() {
        for vertical in [false, true] {
            let mut converter = Converter::default();
            let mut first = None;
            for length in [512., 640., 900.] {
                let bounds = if vertical { graphene::Rect::new(0., 0., 36., length) }
                    else { graphene::Rect::new(0., 0., length, 36.) };
                let outline = gsk::RoundedRect::from_rect(bounds, 9.72);
                let shadow = gsk::OutsetShadowNode::new(&outline, &gdk::RGBA::new(0., 0., 0., 0.16), 0., 2., 0., 8.);
                let node = converter.frame(shadow.upcast_ref(), 2.);
                assert!(node.is::<gsk::ContainerNode>());
                assert_eq!(converter.compact_shadows.len(), 1);
                let textures: Vec<_> = converter.compact_shadows.values().flat_map(|tiles|
                    tiles.iter().map(|tile| tile.as_ptr() as usize)).collect();
                if let Some(first) = &first { assert_eq!(&textures, first); } else { first = Some(textures); }
                fn check(node: &gsk::RenderNode) {
                    assert!(!matches!(node.node_type(), gsk::RenderNodeType::BlurNode | gsk::RenderNodeType::RepeatNode));
                    for child in node.children() { check(&child); }
                }
                check(&node);
            }
        }
    }

    #[test]
    fn panel_contents_have_one_native_clip_and_preserve_color_nodes() {
        let bounds = graphene::Rect::new(0., 0., 242., 320.);
        let content = gsk::ContainerNode::new(&[
            gsk::ColorNode::new(&gdk::RGBA::WHITE, &bounds).upcast(),
            gsk::ColorNode::new(&gdk::RGBA::BLACK, &graphene::Rect::new(8., 8., 16., 16.)).upcast(),
        ]);
        let node = gsk::RoundedClipNode::new(&content, &gsk::RoundedRect::from_rect(bounds, 9.72));
        let converted = Converter::default().frame(node.upcast_ref(), 1.);
        let clip = converted.downcast_ref::<gsk::RoundedClipNode>().unwrap();
        assert_eq!(clip.child().as_ptr(), content.upcast_ref().as_ptr());
        assert!((clip.clip().corner()[0].width() - 9.72).abs() < 0.0001);
        assert_eq!(clip.child().bounds(), bounds);
    }

    #[test]
    fn panel_clip_corners_follow_the_window_without_reusing_stale_cache_entries() {
        let bounds = graphene::Rect::new(0., 0., 74., 666.);
        let content = gsk::ContainerNode::new(&[
            gsk::ColorNode::new(&gdk::RGBA::WHITE, &bounds).upcast(),
            gsk::ColorNode::new(&gdk::RGBA::BLACK, &graphene::Rect::new(8., 8., 16., 16.)).upcast(),
        ]);
        let clip = gsk::RoundedClipNode::new(content, &gsk::RoundedRect::from_rect(bounds, 9.72));
        let node = gsk::TransformNode::new(clip, Some(&gsk::Transform::new().translate(&graphene::Point::new(6., 48.))));
        for scale in [1., 2.] {
            let mut converter = Converter::default();
            for height in [718., 719., 740., 718.] {
                let result = converter.frame_in_window(node.upcast_ref(), scale, Some([1034., height, 15.]));
                let child = result.downcast_ref::<gsk::TransformNode>().unwrap().child();
                let clip = child.downcast_ref::<gsk::RoundedClipNode>().unwrap().clip();
                let expected = if height < 729. { 9.72f32.max(15. - (height - 714.)) } else { 9.72 };
                assert!((clip.corner()[3].height() - expected).abs() < 0.0001);
                assert!((clip.corner()[0].height() - 9.72).abs() < 0.0001);
            }
        }
    }

    #[test]
    fn bridge_masks_convert_both_children_and_retain_their_mode_across_corner_changes() {
        let mut converter = Converter::default();
        for scale in [1., 1.25, 2.] {
            for radius in [18., 6., 0., 6., 18.] {
                let node = gsk::MaskNode::new(panel(radius), panel(18. - radius), gsk::MaskMode::InvertedAlpha).upcast();
                let converted = converter.frame(&node, scale);
                let mask = converted.downcast_ref::<gsk::MaskNode>().unwrap();
                assert_eq!(mask.mask_mode(), gsk::MaskMode::InvertedAlpha);
                assert_converted_panel(&mask.source());
                assert_converted_panel(&mask.mask());
                assert_eq!(converter.frame(&node, scale).as_ptr(), converted.as_ptr());
            }
        }
    }

    #[test]
    fn effect_wrappers_preserve_parameters_and_convert_panel_corners_and_shadows() {
        let mut converter = Converter::default();
        let matrix = graphene::Matrix::new_scale(0.5, 0.75, 1.);
        let offset = graphene::Vec4::new(0.1, 0.2, 0.3, 0.);
        let color = gsk::ColorMatrixNode::new(panel(0.), &matrix, &offset).upcast();
        let color = converter.frame(&color, 1.25).downcast::<gsk::ColorMatrixNode>().unwrap();
        assert_eq!(color.color_matrix(), matrix);
        assert_eq!(color.color_offset(), offset);
        assert_converted_panel(&color.child());
        let blur = gsk::BlurNode::new(panel(6.), 3.).upcast();
        let blur = converter.frame(&blur, 1.25).downcast::<gsk::BlurNode>().unwrap();
        assert_eq!(blur.radius(), 3.);
        assert_converted_panel(&blur.child());
        let fade = gsk::CrossFadeNode::new(panel(0.), panel(18.), 0.4).upcast();
        let fade = converter.frame(&fade, 1.25).downcast::<gsk::CrossFadeNode>().unwrap();
        assert_eq!(fade.progress(), 0.4);
        assert_converted_panel(&fade.start_child());
        assert_converted_panel(&fade.end_child());
        let blend = gsk::BlendNode::new(panel(0.), panel(18.), gsk::BlendMode::Multiply).upcast();
        let blend = converter.frame(&blend, 1.25).downcast::<gsk::BlendNode>().unwrap();
        assert_eq!(blend.blend_mode(), gsk::BlendMode::Multiply);
        assert_converted_panel(&blend.bottom_child());
        assert_converted_panel(&blend.top_child());
    }
}

fn surface_extent(widget: &gtk::Widget) -> gsk::RenderNode {
    let bounds = graphene::Rect::new(0., 0., widget.width() as f32, widget.height() as f32);
    gsk::ColorNode::new(&gdk::RGBA::TRANSPARENT, &bounds).upcast()
}

fn append_converted(widget: &gtk::Widget, converter: &RefCell<Converter>, content: gtk::Snapshot, snapshot: &gtk::Snapshot) {
    let Some(node) = content.to_node() else {
        return;
    };
    let scale = widget
        .native()
        .and_then(|native| native.surface())
        .map_or_else(|| f64::from(widget.scale_factor()), |surface| surface.scale());
    let window = widget.native().and_then(|native| native.downcast::<crate::window::Window>().ok())
        .map(|window| [window.width() as f32, window.height() as f32, window.corner_radius()]);
    snapshot.append_node(converter.borrow_mut().frame_in_window(&node, scale, window));
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
            snapshot.append_node(surface_extent(obj.upcast_ref()));
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

#[cfg(test)]
#[path = "native_window_reference.rs"]
pub(crate) mod native_window_reference;
