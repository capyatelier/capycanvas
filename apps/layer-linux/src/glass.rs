use gtk::gsk;
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
        let mut radii = rect.corner().map(|s| s.width().min(s.height()) / fit * scale);
        let [w, h] = [bounds[2], bounds[3]];
        let [tl, tr, br, bl] = radii;
        let factor = [w / (tl + tr), w / (bl + br), h / (tl + bl), h / (tr + br)]
            .into_iter()
            .filter(|f| f.is_finite())
            .fold(1f32, f32::min);
        if factor < 1. {
            radii = radii.map(|r| r * factor);
        }
        Self {
            bounds,
            radii,
            shape: if round { BackdropRegion::CIRCULAR } else { BackdropRegion::SQUIRCLE },
        }
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

pub fn collect(node: &gsk::RenderNode, surfaces: &[GlassColor], out: &mut Vec<BackdropRegion>) {
    let place = Place { scale: [1.; 2], offset: [0.; 2] };
    let context = Context { place, clip: None, cut: None, round: false };
    visit(node, context, surfaces, out);
}

#[derive(Clone, Copy)]
struct Context {
    place: Place,
    clip: Option<Clip>,
    cut: Option<[f32; 4]>,
    round: bool,
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
    } else if let Some(n) = node.downcast_ref::<gsk::ColorNode>() {
        let color = n.color();
        let rgba = [color.red(), color.green(), color.blue(), color.alpha()];
        if !surfaces.iter().any(|s| s.matches(rgba)) {
            return None;
        }
        let rect = at.place.rect(&n.bounds());
        let rect = at.cut.map_or(Some(rect), |c| intersect(c, rect))?;
        let region = match at.clip {
            Some(c) => c.region(rect),
            None => Some(BackdropRegion { bounds: rect, radii: [0.; 4], shape: BackdropRegion::SQUIRCLE }),
        };
        out.extend(region);
        Some(n.bounds())
    } else if let Some(n) = node.downcast_ref::<gsk::DebugNode>() {
        let round = at.round || n.message() == crate::squircle::KEEP_ROUND;
        visit(&n.child(), Context { round, ..at }, surfaces, out)
    } else if let Some(n) = node.downcast_ref::<gsk::OpacityNode>() {
        visit(&n.child(), at, surfaces, out)
    } else if let Some(n) = node.downcast_ref::<gsk::ShadowNode>() {
        visit(&n.child(), at, surfaces, out)
    } else if let Some(n) = node.downcast_ref::<gsk::IsolationNode>() {
        visit(&n.child(), at, surfaces, out)
    } else {
        None
    }
}

pub fn connector(c: &layer_ui::DrawerConnection, out: &mut Vec<BackdropRegion>) {
    let [xx, yx, xy, yy, x, y] = c.transform;
    let map = |u: f32, v: f32| [xx * u + xy * v + x + c.bounds.x, yx * u + yy * v + y + c.bounds.y];
    let square = |u0: f32, v0: f32, u1: f32, v1: f32| {
        let points = [map(u0, v0), map(u1, v0), map(u1, v1), map(u0, v1)];
        let min = [0, 1].map(|i| points.iter().map(|p| p[i]).fold(f32::INFINITY, f32::min));
        let max = [0, 1].map(|i| points.iter().map(|p| p[i]).fold(f32::NEG_INFINITY, f32::max));
        [min[0], min[1], max[0] - min[0], max[1] - min[1]]
    };
    let [length, depth] = [c.length, c.depth];
    out.push(BackdropRegion {
        bounds: square(0., 0., length, depth),
        radii: [0.; 4],
        shape: BackdropRegion::SQUIRCLE,
    });
    for (edge, r, direction) in [(0., c.radii[0], -1.), (length, c.radii[1], 1.)] {
        if r <= 0. {
            continue;
        }
        let far = edge + direction * r;
        let bounds = square(edge.min(far), depth - r, edge.max(far), depth);
        let center = map(far, depth - r);
        let corner = match (
            (center[0] - bounds[0]).abs() < 0.5,
            (center[1] - bounds[1]).abs() < 0.5,
        ) {
            (true, true) => 0,
            (false, true) => 1,
            (false, false) => 2,
            (true, false) => 3,
        };
        let mut radii = [0.; 4];
        radii[corner] = -r;
        out.push(BackdropRegion { bounds, radii, shape: BackdropRegion::SQUIRCLE });
    }
}
