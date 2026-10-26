use super::*;
use crate::PointerKind;

pub const EDGE_GRAB_DISTANCE: f32 = TILE_SIZE / 3.0;
pub const EDGE_GRAB_CATCH_UP_MS: f32 = 150.0;
const SCROLLING_EDGE_REACH: f32 = 24.0;
const STRIP_EDGE_REACH: f32 = WORKSPACE_SPACING;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeTarget {
    Divider { id: u32, axis: Axis },
    Floating { group: u32, edge: ResizeEdge },
}
impl EdgeTarget {
    fn axes(self) -> [bool; 2] {
        match self {
            Self::Divider { axis, .. } => [axis == Axis::Horizontal, axis == Axis::Vertical],
            Self::Floating { edge: ResizeEdge::Left | ResizeEdge::Right, .. } => [true, false],
            Self::Floating { edge: ResizeEdge::Top | ResizeEdge::Bottom, .. } => [false, true],
            Self::Floating { .. } => [true, true],
        }
    }
    pub fn cursor(self) -> &'static str {
        match self {
            Self::Divider { axis: Axis::Horizontal, .. } => "col-resize",
            Self::Divider { .. } => "row-resize",
            Self::Floating { edge, .. } => edge.cursor(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Reach {
    target: EdgeTarget,
    line: [f32; 2],
    distance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeGrab {
    origin: [f32; 2],
    edges: [Option<Reach>; 2],
    corner: Option<Reach>,
    claimed: Option<Reach>,
    heading: [f32; 2],
    caught: [bool; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeGrabProgress {
    Pending,
    Claimed(EdgeTarget),
    Abandoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeContact {
    Free,
    Pickup,
    Input,
}

fn nearer(a: Option<Reach>, b: Option<Reach>) -> Option<Reach> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.distance < a.distance { b } else { a }),
        (a, b) => a.or(b),
    }
}

fn tab_strip(g: &GroupPlacement) -> Option<Bounds> {
    g.tabs_visible.then_some(Bounds { height: TAB_BAR_HEIGHT.min(g.bounds.height), ..g.bounds })
}

impl ResolvedLayout {
    pub fn edge_grab_at(&self, point: [f32; 2], kind: PointerKind, contact: EdgeContact) -> Option<EdgeGrab> {
        let [x, y] = point;
        let grip = |g: &GroupPlacement| {
            let body = g.body();
            g.footer_grip.is_some_and(|grip| grip.contains(x - g.bounds.x, y - g.bounds.y))
                || g.tiles.as_ref().and_then(|t| t.grip).is_some_and(|grip| grip.contains(x - body.x, y - body.y))
        };
        if contact == EdgeContact::Input
            || self.groups.iter().any(grip)
            || self.collapsed.iter().any(|c| c.grip.contains(x, y))
        {
            return None;
        }
        let (edges, corner) = match self.groups.iter().rev().find(|g| g.floating && g.bounds.contains(x, y)) {
            Some(floating) => self.floating_inside(floating, point, contact),
            None if kind != PointerKind::Touch && self.work_area.contains(x, y) => return None,
            None => {
                let tabs = self.groups.iter().find(|g| !g.floating && tab_strip(g).is_some_and(|b| b.contains(x, y)));
                let (outside, corner) = if kind == PointerKind::Touch && self.work_area.contains(x, y) {
                    self.floating_outside(point)
                } else {
                    ([None; 2], None)
                };
                let docked = self.divider_edges(point, kind, contact, tabs);
                ([0, 1].map(|i| nearer(docked[i], outside[i])), corner.filter(|_| contact == EdgeContact::Free))
            }
        };
        (edges.iter().any(Option::is_some) || corner.is_some())
            .then_some(EdgeGrab { origin: point, edges, corner, claimed: None, heading: [0.; 2], caught: [false; 2] })
    }

    pub fn edge_reset_at(&self, point: [f32; 2], kind: PointerKind) -> Option<u32> {
        match self.edge_grab_at(point, kind, EdgeContact::Free)?.edges[0]?.target {
            EdgeTarget::Divider { id, .. } => self.dividers.iter().any(|d| d.id == id && d.owner_side.is_some()).then_some(id),
            EdgeTarget::Floating { .. } => None,
        }
    }

    fn side_reach(&self, d: &Divider, side: Edge, [x, y]: [f32; 2], kind: PointerKind) -> f32 {
        if self.work_area.contains(x, y) {
            if kind == PointerKind::Touch { TILE_SIZE } else { 0. }
        } else if d.owner_side.is_some_and(|owner| owner != side) {
            0.
        } else if d.axis == Axis::Horizontal {
            TILE_SIZE
        } else if self.groups.iter().any(|g| !g.floating && tab_strip(g).is_some_and(|b| b.contains(x, y))) {
            STRIP_EDGE_REACH
        } else if self.groups.iter().find(|g| !g.floating && g.bounds.contains(x, y)).is_some_and(|g| !g.scrolls) {
            TILE_SIZE
        } else {
            SCROLLING_EDGE_REACH
        }
    }

    fn divider_edges(&self, point: [f32; 2], kind: PointerKind, contact: EdgeContact, tabs: Option<&GroupPlacement>) -> [Option<Reach>; 2] {
        let mut edges = [None; 2];
        for d in self.dividers.iter().filter(|d| !d.fixed) {
            let along = usize::from(d.axis == Axis::Vertical);
            let [start, extent, cross, length] = if along == 0 {
                [d.bounds.x, d.bounds.width, d.bounds.y, d.bounds.height]
            } else {
                [d.bounds.y, d.bounds.height, d.bounds.x, d.bounds.width]
            };
            if (along == 0 && (contact != EdgeContact::Free || tabs.is_some()))
                || !(cross..=cross + length).contains(&point[1 - along])
            {
                continue;
            }
            let low = point[along] < start;
            let distance = if low { start - point[along] } else { (point[along] - start - extent).max(0.) };
            let side = match (along, low) {
                (0, true) => Edge::Left,
                (0, false) => Edge::Right,
                (_, true) => Edge::Top,
                _ => Edge::Bottom,
            };
            let mut probe = point;
            probe[along] = if low { start - 1. } else { start + extent + 1. };
            if distance > self.side_reach(d, side, probe, kind)
                || along == 1 && tabs.is_some_and(|g| (g.bounds.y - start - extent).abs() >= 0.5 || point[0] >= g.bounds.x + g.bounds.width - TILE_SIZE)
            {
                continue;
            }
            let mut line = point;
            line[along] = start + extent * 0.5;
            let target = EdgeTarget::Divider { id: d.id, axis: d.axis };
            edges[along] = nearer(edges[along], Some(Reach { target, line, distance }));
        }
        edges
    }

    fn floating_inside(&self, g: &GroupPlacement, [x, y]: [f32; 2], contact: EdgeContact) -> ([Option<Reach>; 2], Option<Reach>) {
        let b = g.bounds;
        if tab_strip(g).is_some_and(|strip| strip.contains(x, y)) {
            return ([None; 2], None);
        }
        let side = [(ResizeEdge::Left, x - b.x, b.x), (ResizeEdge::Right, b.x + b.width - x, b.x + b.width)]
            .into_iter()
            .filter(|(_, distance, _)| *distance <= TILE_SIZE)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let bottom_reach = if g.scrolls { SCROLLING_EDGE_REACH } else { TILE_SIZE };
        let bottom = (b.y + b.height - y <= bottom_reach).then_some(b.y + b.height);
        self.floating_reach(g.id, [x, y], side.map(|(edge, distance, line)| (edge, distance.max(0.), line)), bottom, contact)
    }

    fn floating_outside(&self, [x, y]: [f32; 2]) -> ([Option<Reach>; 2], Option<Reach>) {
        self.groups.iter().filter(|g| g.floating).fold(([None; 2], None), |(edges, corner), g| {
            let b = g.bounds;
            let top = b.y + tab_strip(g).map_or(0., |strip| strip.height);
            let side = [(ResizeEdge::Left, b.x - x, b.x), (ResizeEdge::Right, x - b.x - b.width, b.x + b.width)]
                .into_iter()
                .find(|(_, distance, _)| (0.0..=TILE_SIZE).contains(distance) && (top..=b.y + b.height + TILE_SIZE).contains(&y));
            let below = y - b.y - b.height;
            let bottom = ((0.0..=TILE_SIZE).contains(&below) && (b.x - TILE_SIZE..=b.x + b.width + TILE_SIZE).contains(&x))
                .then_some(b.y + b.height);
            let (found, found_corner) = self.floating_reach(g.id, [x, y], side, bottom, EdgeContact::Free);
            ([0, 1].map(|i| nearer(edges[i], found[i])), nearer(corner, found_corner))
        })
    }

    fn floating_reach(
        &self,
        group: u32,
        [x, y]: [f32; 2],
        side: Option<(ResizeEdge, f32, f32)>,
        bottom: Option<f32>,
        contact: EdgeContact,
    ) -> ([Option<Reach>; 2], Option<Reach>) {
        let reach = |edge, line, distance| Reach { target: EdgeTarget::Floating { group, edge }, line, distance };
        let below = bottom.map(|line| reach(ResizeEdge::Bottom, [x, line], (y - line).abs()));
        let beside = side.filter(|_| contact == EdgeContact::Free).map(|(edge, distance, line)| reach(edge, [line, y], distance));
        let corner = beside.zip(below).map(|(side, bottom)| {
            let edge = if side.target == (EdgeTarget::Floating { group, edge: ResizeEdge::Left }) { ResizeEdge::BottomLeft } else { ResizeEdge::BottomRight };
            reach(edge, [side.line[0], bottom.line[1]], side.distance.min(bottom.distance))
        });
        if corner.is_some() { ([None; 2], corner) } else { ([beside, below], None) }
    }
}

impl EdgeGrab {
    pub fn resizes(self, axis: Axis) -> bool {
        let along = usize::from(axis == Axis::Vertical);
        self.edges[along].is_some() || self.corner.is_some()
    }

    pub fn track(&mut self, point: [f32; 2]) -> EdgeGrabProgress {
        let travel = [0, 1].map(|i| (point[i] - self.origin[i]).abs());
        let along = usize::from(travel[1] > travel[0]);
        if self.claimed.is_none() {
            self.claimed = match self.corner {
                Some(corner) => (travel[0].hypot(travel[1]) >= EDGE_GRAB_DISTANCE).then_some(corner),
                None => self.edges[along].filter(|_| travel[along] >= EDGE_GRAB_DISTANCE && travel[along] >= travel[1 - along] * 2.0),
            };
            self.heading = [0, 1].map(|i| (point[i] - self.origin[i]).signum());
        }
        match self.claimed {
            Some(reach) => EdgeGrabProgress::Claimed(reach.target),
            None if travel[along] >= EDGE_GRAB_DISTANCE => EdgeGrabProgress::Abandoned,
            None => EdgeGrabProgress::Pending,
        }
    }

    pub fn anchor(self, point: [f32; 2]) -> [f32; 2] {
        self.clone().position(point, 0.)
    }

    pub fn position(&mut self, point: [f32; 2], elapsed_ms: f32) -> [f32; 2] {
        let Some(reach) = self.claimed else { return point };
        let t = (elapsed_ms / EDGE_GRAB_CATCH_UP_MS).clamp(0., 1.);
        let eased = 1. - (1. - t).powi(3);
        let axes = reach.target.axes();
        let mut position = point;
        for i in (0..2).filter(|i| axes[*i]) {
            self.caught[i] |= (point[i] - reach.line[i]) * self.heading[i] >= 0.;
            position[i] = if self.caught[i] { reach.line[i] + (point[i] - reach.line[i]) * eased } else { reach.line[i] };
        }
        position
    }
}

#[cfg(test)]
#[path = "edge_grab_tests.rs"]
mod tests;
