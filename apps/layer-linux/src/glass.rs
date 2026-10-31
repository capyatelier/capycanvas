use gtk::{gsk, prelude::*};
use layer_render_wgpu::BackdropRegion;
use layer_ui::GlassColor;

#[derive(Clone, Copy)]
struct Place {
    scale: [f32; 2],
    offset: [f32; 2],
}

impl Place {
    fn rect(self, r: &gtk::graphene::Rect) -> [f32; 4] {
        [
            r.x() * self.scale[0] + self.offset[0],
            r.y() * self.scale[1] + self.offset[1],
            r.width() * self.scale[0],
            r.height() * self.scale[1],
        ]
    }
}

#[derive(Clone, Copy)]
struct Clip {
    bounds: [f32; 4],
    radii: [f32; 4],
    shape: [f32; 4],
}

impl Clip {
    fn new(place: Place, rect: &gsk::RoundedRect, round: bool) -> Self {
        let scale = place.scale[0].min(place.scale[1]);
        let bounds = place.rect(rect.bounds());
        let fit = if round { 1. } else { crate::squircle::CORNER_FIT };
        let radii = rect.corner().map(|s| s.width().min(s.height()) / fit * scale);
        let shape = if round { BackdropRegion::CIRCULAR } else { BackdropRegion::SQUIRCLE };
        let BackdropRegion { radii, .. } = BackdropRegion::rounded(bounds, radii, shape);
        Self { bounds, radii, shape }
    }

    fn region(&self, rect: [f32; 4]) -> Option<BackdropRegion> {
        let [x0, y0] = [rect[0].max(self.bounds[0]), rect[1].max(self.bounds[1])];
        let x1 = (rect[0] + rect[2]).min(self.bounds[0] + self.bounds[2]);
        let y1 = (rect[1] + rect[3]).min(self.bounds[1] + self.bounds[3]);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let near = |a: f32, b: f32| (a - b).abs() < 0.5;
        let [cx0, cy0] = [self.bounds[0], self.bounds[1]];
        let [cx1, cy1] = [cx0 + self.bounds[2], cy0 + self.bounds[3]];
        let corners = [
            near(x0, cx0) && near(y0, cy0),
            near(x1, cx1) && near(y0, cy0),
            near(x1, cx1) && near(y1, cy1),
            near(x0, cx0) && near(y1, cy1),
        ];
        Some(BackdropRegion {
            bounds: [x0, y0, x1 - x0, y1 - y0],
            radii: std::array::from_fn(|i| if corners[i] { self.radii[i] } else { 0. }),
            shape: self.shape,
        })
    }
}

const GLASS_ARC: &str = "glass-arc";
const GLASS_BUBBLE: &str = "glass-color-bubble";
const GLASS_SURFACE: &str = "glass-surface";

pub fn surface(node: gsk::RenderNode) -> gsk::RenderNode {
    gsk::DebugNode::new(node, GLASS_SURFACE.into()).upcast()
}

pub fn color_bubble(node: gsk::RenderNode, layout: layer_ui::ColorBubbleLayout) -> gsk::RenderNode {
    gsk::DebugNode::new(node, format!("{GLASS_BUBBLE} {}", layout.diameter).into()).upcast()
}

pub fn arc(node: gsk::RenderNode, center: [f32; 2], radius: f32, band: f32, degrees: [f32; 2]) -> gsk::RenderNode {
    let [start, sweep] = degrees.map(f32::to_radians);
    gsk::DebugNode::new(node, format!("{GLASS_ARC} {} {} {radius} {band} {start} {sweep}", center[0], center[1]).into()).upcast()
}

fn glass_stroke(node: &gsk::RenderNode, surfaces: &[GlassColor]) -> bool {
    node.downcast_ref::<gsk::StrokeNode>().and_then(|s| s.child().downcast::<gsk::ColorNode>().ok()).is_some_and(|c| {
        let color = c.color();
        surfaces.iter().any(|s| s.matches([color.red(), color.green(), color.blue(), color.alpha()]))
    })
}

pub fn collect(node: &gsk::RenderNode, surfaces: &[GlassColor], out: &mut Vec<BackdropRegion>) {
    let place = Place { scale: [1.; 2], offset: [0.; 2] };
    let context = Context { place, clip: None, cut: None, round: false, opacity: 1. };
    visit(node, context, surfaces, out);
}

pub fn opacity(node: &gsk::RenderNode) -> f32 {
    let mut node = node.clone();
    let mut opacity = 1.;
    loop {
        node = if let Some(n) = node.downcast_ref::<gsk::OpacityNode>() {
            opacity *= n.opacity(); n.child()
        } else if let Some(n) = node.downcast_ref::<gsk::TransformNode>() { n.child()
        } else if let Some(n) = node.downcast_ref::<gsk::ClipNode>() { n.child()
        } else if let Some(n) = node.downcast_ref::<gsk::RoundedClipNode>() { n.child()
        } else if let Some(n) = node.downcast_ref::<gsk::ShadowNode>() { n.child()
        } else if let Some(n) = node.downcast_ref::<gsk::DebugNode>() { n.child()
        } else if let Some(n) = node.downcast_ref::<gsk::IsolationNode>() { n.child()
        } else { return opacity; };
    }
}

#[derive(Clone, Copy)]
struct Context {
    place: Place,
    clip: Option<Clip>,
    cut: Option<[f32; 4]>,
    round: bool,
    opacity: f32,
}

fn intersect(a: [f32; 4], b: [f32; 4]) -> Option<[f32; 4]> {
    let [x0, y0] = [a[0].max(b[0]), a[1].max(b[1])];
    let [x1, y1] = [(a[0] + a[2]).min(b[0] + b[2]), (a[1] + a[3]).min(b[1] + b[3])];
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

fn contains(outer: &gtk::graphene::Rect, inner: &gtk::graphene::Rect) -> bool {
    inner.x() >= outer.x() - 0.5
        && inner.y() >= outer.y() - 0.5
        && inner.x() + inner.width() <= outer.x() + outer.width() + 0.5
        && inner.y() + inner.height() <= outer.y() + outer.height() + 0.5
}

fn visit(
    node: &gsk::RenderNode,
    at: Context,
    surfaces: &[GlassColor],
    out: &mut Vec<BackdropRegion>,
) -> Option<gtk::graphene::Rect> {
    if let Some(n) = node.downcast_ref::<gsk::ContainerNode>() {
        let mut found: Vec<gtk::graphene::Rect> = Vec::new();
        for i in 0..n.n_children() {
            let child = n.child(i);
            if found.iter().any(|f| contains(f, &child.bounds())) {
                continue;
            }
            found.extend(visit(&child, at, surfaces, out));
        }
        None
    } else if let Some(n) = node.downcast_ref::<gsk::TransformNode>() {
        let transform = n.transform();
        if transform.category() < gsk::TransformCategory::_2dAffine {
            return None;
        }
        let (sx, sy, dx, dy) = transform.to_affine();
        let place = Place {
            scale: [at.place.scale[0] * sx, at.place.scale[1] * sy],
            offset: [
                at.place.offset[0] + dx * at.place.scale[0],
                at.place.offset[1] + dy * at.place.scale[1],
            ],
        };
        visit(&n.child(), Context { place, ..at }, surfaces, out).map(|r| {
            gtk::graphene::Rect::new(r.x() * sx + dx, r.y() * sy + dy, r.width() * sx, r.height() * sy)
        })
    } else if let Some(n) = node.downcast_ref::<gsk::RoundedClipNode>() {
        let clip = if n.clip().is_rectilinear() {
            at.clip
        } else {
            Some(Clip::new(at.place, &n.clip(), at.round))
        };
        let cut = at.cut.map_or(Some(at.place.rect(n.clip().bounds())), |c| intersect(c, at.place.rect(n.clip().bounds())));
        let cut = cut?;
        visit(&n.child(), Context { clip, cut: Some(cut), ..at }, surfaces, out)
    } else if let Some(n) = node.downcast_ref::<gsk::ClipNode>() {
        let rect = at.place.rect(&n.clip());
        let cut = at.cut.map_or(Some(rect), |c| intersect(c, rect))?;
        visit(&n.child(), Context { cut: Some(cut), ..at }, surfaces, out)
    } else if node.is::<gsk::ColorNode>()
        || node.downcast_ref::<gsk::DebugNode>().is_some_and(|n| n.message() == GLASS_SURFACE)
    {
        if let Some(n) = node.downcast_ref::<gsk::ColorNode>() {
            let color = n.color();
            if color.alpha() * at.opacity == 1. { return Some(n.bounds()); }
            let rgba = [color.red(), color.green(), color.blue(), color.alpha()];
            if !surfaces.iter().any(|s| s.matches(rgba)) {
                return None;
            }
        }
        let rect = at.place.rect(&node.bounds());
        let rect = at.cut.map_or(Some(rect), |c| intersect(c, rect))?;
        let region = match at.clip {
            Some(c) => c.region(rect),
            None => Some(BackdropRegion { bounds: rect, radii: [0.; 4], shape: BackdropRegion::SQUIRCLE }),
        };
        out.extend(region.map(|r| r.with_strength(at.opacity)));
        Some(node.bounds())
    } else if let Some(n) = node.downcast_ref::<gsk::DebugNode>()
        && let Some(size) = n.message().strip_prefix(GLASS_BUBBLE).and_then(|s| s.trim().parse::<f32>().ok())
    {
        let child = n.child();
        let fill = child.downcast_ref::<gsk::FillNode>()?.child().downcast::<gsk::ColorNode>().ok()?;
        let c = fill.color();
        if !surfaces.iter().any(|s| s.matches([c.red(), c.green(), c.blue(), c.alpha()])) { return None; }
        let layout = layer_ui::ColorBubbleLayout::new(size, true);
        let arc = layout.intensity?;
        let scale = at.place.scale[0].min(at.place.scale[1]);
        let center = at.place.offset;
        let cap = (arc.width * 0.5 + layout.margin()) * scale;
        let p = arc.point(1.);
        out.push(BackdropRegion::color_bubble(center, size * 0.5 * scale, arc.radius * scale, cap, p[1].atan2(p[0])).with_strength(at.opacity));
        None
    } else if let Some(n) = node.downcast_ref::<gsk::DebugNode>()
        && let Some(arc) = n.message().strip_prefix(GLASS_ARC)
    {
        let values: Vec<f32> = arc.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        if let [x, y, radius, band, start, sweep] = values[..] && glass_stroke(&n.child(), surfaces) {
            let Place { scale, offset } = at.place;
            let center = [x * scale[0] + offset[0], y * scale[1] + offset[1]];
            let scale = scale[0].min(scale[1]);
            out.push(BackdropRegion::arc(center, radius * scale, band * scale, start, sweep).with_strength(at.opacity));
        }
        None
    } else if let Some(n) = node.downcast_ref::<gsk::DebugNode>() {
        let round = at.round || n.message() == crate::squircle::KEEP_ROUND;
        visit(&n.child(), Context { round, ..at }, surfaces, out)
    } else if let Some(n) = node.downcast_ref::<gsk::OpacityNode>() {
        visit(&n.child(), Context { opacity: at.opacity * n.opacity(), ..at }, surfaces, out)
    } else if let Some(n) = node.downcast_ref::<gsk::ShadowNode>() {
        visit(&n.child(), at, surfaces, out)
    } else if let Some(n) = node.downcast_ref::<gsk::IsolationNode>() {
        visit(&n.child(), at, surfaces, out)
    } else if let Some(n) = node.downcast_ref::<gsk::MaskNode>() {
        visit(&n.source(), at, surfaces, out)
    } else {
        None
    }
}

pub fn connector(c: &layer_ui::DrawerConnection, out: &mut Vec<BackdropRegion>) {
    out.extend(c.glass().into_iter().map(|(bounds, radii)| BackdropRegion { bounds, radii, shape: BackdropRegion::SQUIRCLE }));
}

#[derive(Default)]
pub(crate) struct Chrome {
    revision: std::cell::Cell<u64>,
    frames: std::cell::RefCell<std::collections::VecDeque<(u64, i64, gsk::RenderNode)>>,
}
pub(crate) const CHROME_HOLD_US: i64 = 100_000;
impl Chrome {
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }
    pub fn compose(&self, node: gsk::RenderNode, changed: bool, presented: Option<u64>, now: i64) -> (gsk::RenderNode, u64, bool) {
        let mut frames = self.frames.borrow_mut();
        if changed || frames.is_empty() {
            if changed {
                self.revision.set(self.revision.get() + 1);
            }
            frames.push_back((self.revision.get(), now, node));
        } else {
            frames.back_mut().unwrap().2 = node;
        }
        let latest = frames.len() - 1;
        let shown = presented
            .and_then(|presented| frames.iter().rposition(|(revision, ..)| *revision <= presented))
            .filter(|&index| index < latest && now - frames[index + 1].1 < CHROME_HOLD_US)
            .unwrap_or(latest);
        frames.drain(..shown);
        let (revision, _, node) = frames.front().unwrap().clone();
        (node, revision, frames.len() > 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faded_glass_and_glass_arcs_become_matching_backdrops() {
        let glass = GlassColor([0.9, 0.9, 0.9, 0.6]);
        let tint = gtk::gdk::RGBA::new(0.9, 0.9, 0.9, 0.6);
        let square = gtk::graphene::Rect::new(10., 10., 40., 40.);
        let fill = gsk::ColorNode::new(&tint, &square);
        let faded = gsk::OpacityNode::new(gsk::RoundedClipNode::new(fill, &gsk::RoundedRect::from_rect(square, 20.)), 0.25);
        let path = gsk::PathBuilder::new();
        path.add_circle(&gtk::graphene::Point::new(100., 100.), 50.);
        let band = gsk::StrokeNode::new(gsk::ColorNode::new(&tint, &gtk::graphene::Rect::new(36., 36., 128., 128.)), &path.to_path(), &gsk::Stroke::new(28.));
        let moved = gsk::TransformNode::new(arc(band.upcast(), [100., 100.], 50., 28., [-90., 180.]), Some(&gsk::Transform::new().translate(&gtk::graphene::Point::new(5., 0.))));
        let native = gsk::CairoNode::new(&gtk::graphene::Rect::new(150., 10., 40., 40.));
        let cr = native.draw_context();
        cr.set_source_rgba(0.9, 0.9, 0.9, 0.6);
        cr.paint().unwrap();
        drop(cr);
        let panel = gsk::OpacityNode::new(surface(native.upcast()), 0.5);
        let root = gsk::ContainerNode::new(&[faded.upcast(), moved.upcast(), panel.upcast()]);
        let mut out = Vec::new();
        collect(&root.upcast(), &[glass], &mut out);
        assert_eq!(out.len(), 3, "{out:?}");
        assert_eq!(out[0].shape[1], 0.25, "a faded glass fill blurs as faintly as it is drawn");
        let expected = BackdropRegion::arc([105., 100.], 50., 28., -std::f32::consts::FRAC_PI_2, std::f32::consts::PI);
        assert_eq!(out[1], expected, "a glass arc keeps its own shape");
        assert_eq!(out[2], BackdropRegion::rounded([150., 10., 40., 40.], [0.; 4], BackdropRegion::SQUIRCLE).with_strength(0.5), "native surface paint retains its rectangular backdrop and fade");
    }

    #[test]
    fn chrome_shows_the_committed_revision_until_the_canvas_reports_or_the_hold_expires() {
        let node = |shade: f32| -> gsk::RenderNode {
            gsk::ColorNode::new(&gtk::gdk::RGBA::new(shade, 0., 0., 1.), &gtk::graphene::Rect::new(0., 0., 1., 1.)).upcast()
        };
        let shade = |node: &gsk::RenderNode| node.downcast_ref::<gsk::ColorNode>().unwrap().color().red();
        let chrome = Chrome::default();
        let shown = |(node, revision, waiting): (gsk::RenderNode, u64, bool)| (shade(&node), revision, waiting);
        assert_eq!(shown(chrome.compose(node(0.), false, Some(0), 0)), (0., 0, false));
        assert_eq!(shown(chrome.compose(node(0.25), true, Some(0), 10)), (0., 0, true));
        assert_eq!(shown(chrome.compose(node(0.5), false, Some(0), 20)), (0., 0, true));
        assert_eq!(shown(chrome.compose(node(0.5), false, Some(1), 30)), (0.5, 1, false));
        assert_eq!(shown(chrome.compose(node(0.75), true, Some(1), 40)), (0.5, 1, true));
        assert_eq!(shown(chrome.compose(node(0.75), false, Some(1), 40 + CHROME_HOLD_US)), (0.75, 2, false));
        assert_eq!(shown(chrome.compose(node(1.), true, None, 50 + CHROME_HOLD_US)), (1., 3, false));
    }
}
