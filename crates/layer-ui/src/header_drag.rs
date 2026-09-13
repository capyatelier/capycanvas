//! Window-bar drag policy. Slots are based on grab-time geometry, never on the
//! animated neighbors. A drag publishes one edit on release, none during motion.
use crate::{
    Bounds, HeaderAction, HeaderGeometry, HeaderItem, HeaderLayout, HeaderMetric, HeaderZone,
    TabHit, tab_drag::TabDrag,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderDragSource {
    Item(u32),
    Component(HeaderItem),
    Tools,
}

pub struct HeaderDrag {
    layout: HeaderLayout,
    source: HeaderDragSource,
    geometry: HeaderGeometry,
    metrics: Vec<HeaderMetric>,
    width: f32,
    insets: [f32; 2],
    press: [f32; 2],
    grab: Bounds,
    slide: Option<(HeaderZone, TabDrag)>,
    detached: bool,
}

#[derive(Clone, Debug)]
pub struct HeaderDragPreview {
    pub geometry: HeaderGeometry,
    pub held: Bounds,
    pub detached: bool,
    pub target: Option<(HeaderZone, Option<u32>)>,
    pub action: Option<HeaderAction>,
}

impl HeaderGeometry {
    /// Gaps between regions belong to the closest region, not dead drop zones.
    /// Native controls outside these bounds are never customization targets.
    pub fn destination(&self, point: [f32; 2], height: f32) -> Option<(HeaderZone, Option<u32>)> {
        if !point.into_iter().all(f32::is_finite) || point[1] < 0. || point[1] > height {
            return None;
        }
        let left = self.zones[0].x;
        let right = self.zones[2].x + self.zones[2].width;
        if !left.is_finite()
            || !right.is_finite()
            || right <= left
            || point[0] < left
            || point[0] > right
        {
            return None;
        }
        let zone = HeaderZone::ALL.into_iter().min_by(|a, b| {
            let distance = |z: HeaderZone| {
                let b = self.zones[z.index()];
                (b.x - point[0]).max(point[0] - b.x - b.width).max(0.)
            };
            distance(*a).total_cmp(&distance(*b))
        })?;
        let bounds = self.zones[zone.index()];
        let before = self
            .items
            .iter()
            .filter(|m| bounds.contains(m.bounds.x + m.bounds.width / 2., m.bounds.y + 1.))
            .find(|m| point[0] < m.bounds.x + m.bounds.width / 2.)
            .map(|m| m.id);
        Some((zone, before))
    }
}

impl HeaderLayout {
    /// One arrow press crosses exactly one slot, including a region boundary.
    pub fn step(&self, id: u32, forward: bool) -> Option<HeaderAction> {
        let (zone, index) = self.location(id)?;
        let entries = &self.zones[zone.index()];
        let (zone, before) = if forward {
            if index + 1 < entries.len() {
                (zone, entries.get(index + 2).map(|e| e.id))
            } else {
                let next = *HeaderZone::ALL.get(zone.index() + 1)?;
                (next, self.zones[next.index()].first().map(|e| e.id))
            }
        } else if index > 0 {
            (zone, Some(entries[index - 1].id))
        } else {
            (*HeaderZone::ALL.get(zone.index().checked_sub(1)?)?, None)
        };
        Some(HeaderAction::Move { id, zone, before })
    }
}

impl HeaderDrag {
    pub fn new(
        layout: &HeaderLayout,
        source: HeaderDragSource,
        geometry: HeaderGeometry,
        metrics: Vec<HeaderMetric>,
        width: f32,
        insets: [f32; 2],
        press: [f32; 2],
        grab: Bounds,
    ) -> Option<Self> {
        if ![
            width,
            press[0],
            press[1],
            grab.x,
            grab.y,
            grab.width,
            grab.height,
            insets[0],
            insets[1],
        ]
        .into_iter()
        .all(f32::is_finite)
            || grab.width <= 0.
            || grab.height <= 0.
            || width <= 0.
            || insets.into_iter().any(|v| v < 0.)
            || geometry.zones[2].x + geometry.zones[2].width <= geometry.zones[0].x
        {
            return None;
        }
        let slide = if let HeaderDragSource::Item(id) = source {
            let (zone, _) = layout.location(id)?;
            let items = geometry
                .items
                .iter()
                .filter(|m| layout.location(m.id).is_some_and(|(z, _)| z == zone))
                .collect::<Vec<_>>();
            let index = items.iter().position(|m| m.id == id)?;
            let tabs = items
                .iter()
                .enumerate()
                .map(|(index, m)| TabHit {
                    group: zone.index() as u32,
                    index,
                    bounds: m.bounds,
                })
                .collect::<Vec<_>>();
            Some((
                zone,
                TabDrag::new(
                    zone.index() as u32,
                    index,
                    press,
                    &tabs,
                    geometry.zones[zone.index()],
                )?,
            ))
        } else {
            if let HeaderDragSource::Component(item) = source
                && item.singleton()
                && layout.entries().any(|e| e.item == item)
            {
                return None;
            }
            if layout.entries().count() >= 128 {
                return None;
            }
            None
        };
        Some(Self {
            layout: layout.clone(),
            source,
            geometry,
            metrics,
            width,
            insets,
            press,
            grab,
            slide,
            detached: !matches!(source, HeaderDragSource::Item(_)),
        })
    }

    pub fn is_current(&self, layout: &HeaderLayout) -> bool {
        &self.layout == layout
    }

    pub fn preview(&mut self, point: [f32; 2]) -> Option<HeaderDragPreview> {
        if !point.into_iter().all(f32::is_finite) {
            return None;
        }
        let height = self.layout.size.height();
        let left = self.geometry.zones[0].x;
        let right = self.geometry.zones[2].x + self.geometry.zones[2].width;
        // Native slop decides pickup. This separate distance decides tear-off.
        // A small vertical wobble still slides on the bar; returning inside
        // the bar reattaches the same contact without changing the grab offset.
        let distance = (-point[1])
            .max(point[1] - height)
            .max(left - point[0])
            .max(point[0] - right);
        if !self.detached && distance > self.layout.size.tile() / 2. {
            self.detached = true;
        }
        let direct = self.geometry.destination(point, height);
        if self.detached && direct.is_some() {
            self.detached = false;
        }
        let mut target = if self.detached {
            None
        } else {
            self.geometry.destination(
                [point[0].clamp(left, right), self.grab.y.min(height / 2.)],
                height,
            )
        };
        if let (Some((zone, before)), Some((origin, slide))) = (&mut target, &self.slide)
            && zone == origin
        {
            let preview = slide.preview(point)?;
            // Tab insertion is an index before removal. Use original IDs so
            // unequal widths and backtracking cannot oscillate on animation.
            *before = self.layout.zones[zone.index()]
                .get(preview.insertion)
                .map(|e| e.id);
        }
        let mut layout = self.layout.clone();
        let mut metrics = self.metrics.clone();
        let action = match (self.source, target) {
            (HeaderDragSource::Item(id), Some((zone, before))) => {
                layout.move_item(id, zone, before).ok()?;
                Some(HeaderAction::Move { id, zone, before })
            }
            (HeaderDragSource::Item(id), None) => {
                layout.remove(id).ok()?;
                Some(HeaderAction::Remove { id })
            }
            (source, Some((zone, before))) => {
                let item = match source {
                    HeaderDragSource::Component(item) => item,
                    _ => HeaderItem::Space,
                };
                layout.add(zone, before, &[item]).ok()?;
                let id = layout
                    .entries()
                    .find(|e| self.layout.entry(e.id).is_err())?
                    .id;
                metrics.push(HeaderMetric {
                    id,
                    width: self.layout.size.tile() + 20.,
                    compact: self.layout.size.tile() + 20.,
                });
                Some(match source {
                    HeaderDragSource::Component(item) => HeaderAction::Add { zone, before, item },
                    _ => HeaderAction::InsertTools { zone, before },
                })
            }
            (_, None) => None,
        };
        let held = Bounds {
            x: if self.detached {
                self.grab.x + point[0] - self.press[0]
            } else {
                (self.grab.x + point[0] - self.press[0])
                    .clamp(left, (right - self.grab.width).max(left))
            },
            y: if self.detached {
                self.grab.y + point[1] - self.press[1]
            } else {
                6.
            },
            ..self.grab
        };
        Some(HeaderDragPreview {
            geometry: layout.resolve(self.width, self.insets, &metrics, true),
            held,
            detached: self.detached,
            target,
            action,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(source: HeaderDragSource, offset: f32) -> (HeaderLayout, HeaderDrag) {
        let layout = HeaderLayout::painter();
        let metrics = layout
            .entries()
            .map(|e| HeaderMetric {
                id: e.id,
                width: 68.,
                compact: 68.,
            })
            .collect::<Vec<_>>();
        let geometry = layout.resolve(1800., [0., 72.], &metrics, true);
        let grab = match source {
            HeaderDragSource::Item(id) => {
                geometry.items.iter().find(|m| m.id == id).unwrap().bounds
            }
            _ => Bounds {
                x: 24.,
                y: 160.,
                width: 100.,
                height: 32.,
            },
        };
        let press = [grab.x + offset, grab.y + 12.];
        let drag = HeaderDrag::new(
            &layout,
            source,
            geometry,
            metrics,
            1800.,
            [0., 72.],
            press,
            grab,
        )
        .unwrap();
        (layout, drag)
    }
    #[test]
    fn slide_tearoff_reattach_and_cancel_keep_original_layout_and_grab() {
        for offset in [2., 34., 66.] {
            let (layout, mut drag) = fixture(HeaderDragSource::Item(2), offset);
            let press = drag.press;
            let stationary = drag.preview(press).unwrap();
            let shifted = drag.preview([press[0] + 45., press[1] + 12.]).unwrap();
            assert!(!shifted.detached);
            assert_eq!(shifted.held.y, 6.);
            let neighbor = |p: &HeaderDragPreview| {
                p.geometry
                    .items
                    .iter()
                    .find(|m| m.id == 3)
                    .unwrap()
                    .bounds
                    .x
            };
            assert!(neighbor(&shifted) < neighbor(&stationary));
            assert_eq!(
                drag.preview(press).unwrap().geometry.items,
                stationary.geometry.items
            );
            let out = drag.preview([press[0] + 30., 200.]).unwrap();
            assert!(out.detached && out.target.is_none());
            assert_eq!(out.held.x, drag.grab.x + 30.);
            assert_eq!(out.held.y, 188.);
            assert_eq!(out.action, Some(HeaderAction::Remove { id: 2 }));
            assert!(!out.geometry.items.iter().any(|m| m.id == 2));
            assert!(matches!(
                drag.preview(press).unwrap().action,
                Some(HeaderAction::Move { id: 2, .. })
            ));
            assert!(drag.is_current(&layout));
            assert!(drag.preview([f32::NAN, 20.]).is_none());
        }
    }
    #[test]
    fn catalog_drop_tools_and_invalidated_source_are_atomic() {
        for source in [
            HeaderDragSource::Tools,
            HeaderDragSource::Component(HeaderItem::Clock),
        ] {
            let (mut layout, mut drag) = fixture(source, 10.);
            assert!(drag.preview([500., 500.]).unwrap().action.is_none());
            let p = drag.preview([900., 25.]).unwrap();
            assert!(!p.detached && p.target.unwrap().0 == HeaderZone::Center);
            assert!(matches!(
                p.action,
                Some(HeaderAction::InsertTools { .. } | HeaderAction::Add { .. })
            ));
            layout.remove(1).unwrap();
            assert!(!drag.is_current(&layout));
        }
    }
    #[test]
    fn arrows_cross_regions_in_both_directions_and_stop_at_outer_edges() {
        let mut layout = HeaderLayout::painter();
        let id = layout.zones[0].last().unwrap().id;
        let HeaderAction::Move { zone, before, .. } = layout.step(id, true).unwrap() else {
            panic!()
        };
        assert_eq!(zone, HeaderZone::Center);
        layout.move_item(id, zone, before).unwrap();
        assert_eq!(layout.location(id), Some((HeaderZone::Center, 0)));
        let HeaderAction::Move { zone, before, .. } = layout.step(id, false).unwrap() else {
            panic!()
        };
        assert_eq!((zone, before), (HeaderZone::Left, None));
        assert!(layout.step(layout.zones[0][0].id, false).is_none());
        assert!(
            layout
                .step(layout.zones[2].last().unwrap().id, true)
                .is_none()
        );
    }
}
