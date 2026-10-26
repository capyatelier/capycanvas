use super::*;

const FRICTION: f32 = 0.963;
const RETURN: f32 = 0.054;
const PUSH_SPEED: f32 = 0.12;
const MAX_SPEED: f32 = 4.5;
const REBOUND: f32 = 0.25;
const STEP_NS: u64 = 4_166_667;
const CATCH_UP: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shape {
    pub(crate) half: [f32; 2],
    pub(crate) round: bool,
    pub(crate) offset: [f32; 2],
}
impl Shape {
    pub(crate) fn circle(radius: f32) -> Self {
        Self { half: [radius; 2], round: true, offset: [0.; 2] }
    }
    pub(crate) fn rect(size: [f32; 2]) -> Self {
        Self { half: [size[0] * 0.5, size[1] * 0.5], round: false, offset: [0.; 2] }
    }
    pub(crate) fn at(self, offset: [f32; 2]) -> Self {
        Self { offset, ..self }
    }
    fn distance(self, center: [f32; 2], point: [f32; 2]) -> f32 {
        let d = sub(point, add(center, self.offset));
        if self.round {
            length(d) - self.half[0]
        } else {
            length([(d[0].abs() - self.half[0]).max(0.), (d[1].abs() - self.half[1]).max(0.)])
        }
    }
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn scale(a: [f32; 2], s: f32) -> [f32; 2] {
    [a[0] * s, a[1] * s]
}
fn length(a: [f32; 2]) -> f32 {
    a[0].hypot(a[1])
}
fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn sign(v: f32) -> f32 {
    if v < 0. { -1. } else { 1. }
}
fn capped(v: [f32; 2]) -> [f32; 2] {
    let speed = length(v);
    if speed > MAX_SPEED { scale(v, MAX_SPEED / speed) } else { v }
}

fn rect_circle(rect: [f32; 2], shape: Shape, circle: [f32; 2], radius: f32) -> Option<([f32; 2], f32)> {
    let delta = sub(circle, rect);
    let closest = [delta[0].clamp(-shape.half[0], shape.half[0]), delta[1].clamp(-shape.half[1], shape.half[1])];
    let out = sub(delta, closest);
    let distance = length(out);
    if distance > 1e-3 {
        return (distance < radius).then(|| (scale(out, 1. / distance), radius - distance));
    }
    let edge = [shape.half[0] - delta[0].abs(), shape.half[1] - delta[1].abs()];
    Some(if edge[0] < edge[1] { ([sign(delta[0]), 0.], edge[0] + radius) } else { ([0., sign(delta[1])], edge[1] + radius) })
}

fn deepest(a: [f32; 2], sa: &[Shape], b: [f32; 2], sb: &[Shape]) -> Option<([f32; 2], f32)> {
    sa.iter()
        .flat_map(|pa| sb.iter().filter_map(move |pb| penetration(add(a, pa.offset), *pa, add(b, pb.offset), *pb)))
        .max_by(|x, y| x.1.total_cmp(&y.1))
}

fn extent(parts: &[Shape]) -> [[f32; 2]; 2] {
    parts.iter().fold([[f32::MAX; 2], [f32::MIN; 2]], |[lo, hi], p| {
        [
            [lo[0].min(p.offset[0] - p.half[0]), lo[1].min(p.offset[1] - p.half[1])],
            [hi[0].max(p.offset[0] + p.half[0]), hi[1].max(p.offset[1] + p.half[1])],
        ]
    })
}

fn penetration(a: [f32; 2], sa: Shape, b: [f32; 2], sb: Shape) -> Option<([f32; 2], f32)> {
    let gap = WORKSPACE_SPACING;
    let delta = sub(b, a);
    match (sa.round, sb.round) {
        (true, true) => {
            let distance = length(delta);
            let depth = sa.half[0] + sb.half[0] + gap - distance;
            (depth > 0.).then(|| (if distance > 1e-3 { scale(delta, 1. / distance) } else { [1., 0.] }, depth))
        }
        (false, false) => {
            let overlap = [sa.half[0] + sb.half[0] + gap - delta[0].abs(), sa.half[1] + sb.half[1] + gap - delta[1].abs()];
            if overlap[0] <= 0. || overlap[1] <= 0. {
                return None;
            }
            Some(if overlap[0] < overlap[1] { ([sign(delta[0]), 0.], overlap[0]) } else { ([0., sign(delta[1])], overlap[1]) })
        }
        (false, true) => rect_circle(a, sa, b, sb.half[0] + gap),
        (true, false) => rect_circle(b, sb, a, sa.half[0] + gap).map(|(n, d)| (scale(n, -1.), d)),
    }
}

#[derive(Clone, Copy, Debug)]
struct Body {
    id: u32,
    pos: [f32; 2],
    rest: [f32; 2],
    vel: [f32; 2],
    last: [f32; 2],
    shape: Shape,
    kicked: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Desk {
    bodies: Vec<Body>,
    grabbed: Option<u32>,
    grown: Vec<(u32, Vec<Shape>)>,
    homing: bool,
    bounds: Option<Bounds>,
    clock: Option<u64>,
    moved: bool,
    still: bool,
    faded: Vec<u32>,
    target: Option<[f32; 2]>,
}

impl Desk {
    pub(crate) fn matches(&self, items: &[(u32, [f32; 2], Shape)], bounds: Bounds) -> bool {
        self.bounds == Some(bounds)
            && self.bodies.len() == items.len()
            && self.bodies.iter().zip(items).all(|(body, (id, center, shape))| {
                body.id == *id && body.shape == *shape && (body.rest == *center || body.kicked || self.grabbed.is_some())
            })
    }
    pub(crate) fn sync(&mut self, items: &[(u32, [f32; 2], Shape)], bounds: Bounds) {
        self.bounds = Some(bounds);
        let previous = std::mem::take(&mut self.bodies);
        self.bodies = items
            .iter()
            .map(|&(id, center, shape)| match previous.iter().find(|b| b.id == id) {
                Some(body) => Body { shape, rest: if body.kicked || self.grabbed.is_some() { body.rest } else { center }, ..*body },
                None => Body { id, pos: center, rest: center, vel: [0.; 2], last: center, shape, kicked: false },
            })
            .collect();
        if self.grabbed.is_some_and(|id| !self.bodies.iter().any(|b| b.id == id)) {
            self.grabbed = None;
        }
        self.faded.retain(|id| items.iter().any(|item| item.0 == *id));
        self.moved = true;
        self.separate();
    }
    pub(crate) fn still(&mut self, still: bool) {
        self.still = still;
    }
    pub(crate) fn home(&mut self, homing: bool) {
        if self.homing != homing {
            self.homing = homing;
            self.moved = true;
        }
    }
    pub(crate) fn grab(&mut self, id: u32) {
        self.target = None;
        self.grabbed = self.bodies.iter().any(|b| b.id == id).then_some(id);
        if let Some(body) = self.body_mut(id) {
            body.kicked = false;
            body.vel = [0.; 2];
            body.last = body.pos;
        }
    }
    pub(crate) fn catch_up(&mut self) {
        self.target = self.grabbed.and_then(|id| self.center(id));
    }
    pub(crate) fn drag(&mut self, center: [f32; 2]) {
        let Some(id) = self.grabbed else { return };
        if self.target.is_some() && !self.still {
            self.target = Some(center);
            self.moved = true;
            return;
        }
        if let Some(body) = self.body_mut(id) {
            body.pos = center;
            body.rest = center;
        }
        self.moved = true;
        for _ in 0..4 {
            self.separate();
        }
    }
    pub(crate) fn release(&mut self) {
        let still = self.still;
        let target = self.target.take();
        if let Some(id) = self.grabbed.take()
            && let Some(body) = self.body_mut(id)
        {
            body.rest = target.unwrap_or(body.pos);
            body.vel = if still { [0.; 2] } else { capped(body.vel) };
            body.kicked = length(body.vel) > 0.05;
        }
        self.moved = true;
    }
    pub(crate) fn cancel(&mut self, before: Self) {
        let still = self.still;
        *self = before;
        self.still = still;
        self.clock = None;
        self.moved = true;
    }
    pub(crate) fn grow(&mut self, grown: Vec<(u32, Vec<Shape>)>) {
        if self.grown != grown {
            self.grown = grown;
            self.moved = true;
        }
    }
    pub(crate) fn center(&self, id: u32) -> Option<[f32; 2]> {
        self.bodies.iter().find(|b| b.id == id).map(|b| b.pos)
    }
    pub(crate) fn placements(&self) -> Vec<BubblePlacement> {
        self.bodies
            .iter()
            .map(|b| {
                let lifted = self.grabbed == Some(b.id);
                BubblePlacement { id: b.id, center: b.pos, lifted, homing: lifted && self.homing }
            })
            .collect()
    }
    pub(crate) fn faded(&self) -> &[u32] {
        &self.faded
    }
    pub(crate) fn fade(&mut self, stroke: Option<[f32; 2]>) -> bool {
        let Some(point) = stroke else {
            let changed = !self.faded.is_empty();
            self.faded.clear();
            return changed;
        };
        let near: Vec<_> = self
            .bodies
            .iter()
            .filter(|b| !self.faded.contains(&b.id) && self.shape(b).iter().any(|s| s.distance(b.pos, point) <= STROKE_FADE))
            .map(|b| b.id)
            .collect();
        self.faded.extend_from_slice(&near);
        !near.is_empty()
    }
    pub(crate) fn moving(&self) -> bool {
        self.moved || self.grabbed.is_some() || self.target.is_some() || self.bodies.iter().any(|b| b.kicked)
    }
    pub(crate) fn advance(&mut self, now_ns: u64) -> bool {
        let last = *self.clock.get_or_insert(now_ns.saturating_sub(STEP_NS));
        let steps = (now_ns.saturating_sub(last) / STEP_NS).min(32);
        self.clock = Some(if steps == 32 { now_ns } else { last + steps * STEP_NS });
        let before: Vec<_> = self.bodies.iter().map(|b| b.pos).collect();
        for _ in 0..steps {
            self.step();
        }
        if steps > 0 {
            self.moved = self.bodies.iter().zip(before).any(|(b, p)| length(sub(b.pos, p)) > 0.01);
        }
        if !self.moving() {
            self.clock = None;
        }
        self.moving()
    }
    pub(crate) fn settled(&self) -> Vec<(u32, [f32; 2])> {
        let mut desk = self.clone();
        desk.release();
        for _ in 0..2400 {
            if !desk.bodies.iter().any(|b| b.kicked) {
                break;
            }
            desk.step();
        }
        desk.bodies.iter().map(|b| (b.id, if b.kicked { b.pos } else { b.rest })).collect()
    }
    fn body_mut(&mut self, id: u32) -> Option<&mut Body> {
        self.bodies.iter_mut().find(|b| b.id == id)
    }
    fn shape<'a>(&'a self, body: &'a Body) -> &'a [Shape] {
        self.grown.iter().find(|(id, _)| *id == body.id).map_or(std::slice::from_ref(&body.shape), |(_, parts)| parts)
    }
    fn priority(&self, body: &Body) -> u8 {
        if self.grabbed == Some(body.id) {
            3
        } else if self.grown.iter().any(|(id, _)| *id == body.id) {
            2
        } else {
            u8::from(body.kicked)
        }
    }
    fn step(&mut self) {
        let (grabbed, still) = (self.grabbed, self.still);
        if let (Some(id), Some(target)) = (grabbed, self.target) {
            let body = self.body_mut(id).unwrap();
            let towards = sub(target, body.pos);
            body.pos = if length(towards) < 0.5 { target } else { add(body.pos, scale(towards, CATCH_UP)) };
            body.rest = body.pos;
            if body.pos == target {
                self.target = None;
            }
        }
        for body in &mut self.bodies {
            if grabbed == Some(body.id) {
                body.vel = scale(add(body.vel, sub(body.pos, body.last)), 0.5);
                body.last = body.pos;
                continue;
            }
            if body.kicked {
                body.pos = add(body.pos, body.vel);
                body.vel = scale(body.vel, FRICTION);
                if length(body.vel) < 0.05 {
                    body.vel = [0.; 2];
                    body.kicked = false;
                    body.rest = body.pos;
                }
            } else {
                let towards = sub(body.rest, body.pos);
                body.pos = if still || length(towards) < 0.25 { body.rest } else { add(body.pos, scale(towards, RETURN)) };
            }
            body.last = body.pos;
        }
        for _ in 0..4 {
            self.separate();
        }
    }
    fn walled(&self, body: &Body, direction: [f32; 2]) -> bool {
        let Some(bounds) = self.bounds else { return false };
        let [near, far] = extent(self.shape(body));
        let lo = [bounds.x - near[0], bounds.y - near[1]];
        let hi = [bounds.x + bounds.width - far[0], bounds.y + bounds.height - far[1]];
        (0..2).any(|axis| (direction[axis] > 0.5 && body.pos[axis] >= hi[axis] - 0.5) || (direction[axis] < -0.5 && body.pos[axis] <= lo[axis] + 0.5))
    }
    fn separate(&mut self) {
        let count = self.bodies.len();
        for i in 0..count {
            for j in i + 1..count {
                let (a, b) = (self.bodies[i], self.bodies[j]);
                let Some((normal, depth)) = deepest(a.pos, self.shape(&a), b.pos, self.shape(&b)) else { continue };
                let (pa, pb) = (self.priority(&a), self.priority(&b));
                let away = sub(b.pos, a.pos);
                let inward = self.shape(if pa == 2 { &a } else { &b }).len() > 1 && (pa == 2) != (pb == 2) && dot(normal, away) < 0.;
                let normal = if inward { scale(away, 1. / length(away).max(1e-3)) } else { normal };
                let still = self.still;
                let walls = [self.walled(&b, normal), self.walled(&a, scale(normal, -1.))];
                let mut push = |index: usize, direction: [f32; 2], amount: f32, kick: bool| {
                    let body = &mut self.bodies[index];
                    body.pos = add(body.pos, scale(direction, amount));
                    if kick && still {
                        body.rest = body.pos;
                    } else if kick {
                        body.kicked = true;
                        body.vel = capped(add(body.vel, scale(direction, amount * PUSH_SPEED)));
                    }
                };
                if pa == 2 && pb < 2 && walls[0] {
                    push(i, scale(normal, -1.), depth, false);
                } else if pb == 2 && pa < 2 && walls[1] {
                    push(j, normal, depth, false);
                } else if pa > pb {
                    push(j, normal, depth, pa != 2);
                } else if pb > pa {
                    push(i, scale(normal, -1.), depth, pb != 2);
                } else {
                    push(j, normal, depth * 0.5, false);
                    push(i, scale(normal, -1.), depth * 0.5, false);
                }
            }
        }
        let Some(bounds) = self.bounds else { return };
        for index in 0..count {
            let [near, far] = extent(self.shape(&self.bodies[index]));
            let body = &mut self.bodies[index];
            let lo = [bounds.x - near[0], bounds.y - near[1]];
            let hi = [bounds.x + bounds.width - far[0], bounds.y + bounds.height - far[1]];
            for axis in 0..2 {
                let clamped = if lo[axis] > hi[axis] { (lo[axis] + hi[axis]) * 0.5 } else { body.pos[axis].clamp(lo[axis], hi[axis]) };
                if clamped != body.pos[axis] {
                    body.pos[axis] = clamped;
                    body.vel[axis] *= -REBOUND;
                }
            }
        }
    }
}
