use crate::{Bounds, TabHit};
use serde::Serialize;

/// Temporary tab positions. Native hit rectangles remain in their original
/// slots so animation cannot change the insertion decision under the pointer.
#[derive(Clone, Debug, Serialize)]
pub struct TabDragPreview {
    pub bounds: Bounds,
    pub insertion: usize,
    pub offsets: Vec<TabDragOffset>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct TabDragOffset {
    pub index: usize,
    pub x: f32,
}

/// Geometry captured at grab time. Switch points are distances from the press,
/// so variable tab widths and the grab position do not change the halfway rule.
pub(crate) struct TabDrag {
    pub group: u32,
    source: usize,
    press: f32,
    tabs: Vec<TabHit>,
    clip: Bounds,
    switches: Vec<f32>,
}

impl TabDrag {
    pub fn new(
        group: u32,
        source: usize,
        press: [f32; 2],
        tabs: &[TabHit],
        clip: Bounds,
    ) -> Option<Self> {
        let mut tabs: Vec<_> = tabs
            .iter()
            .filter(|tab| tab.group == group)
            .cloned()
            .collect();
        tabs.sort_by_key(|tab| tab.index);
        if ![press[0], clip.x, clip.width]
            .into_iter()
            .all(f32::is_finite)
            || clip.width <= 0.
            || tabs.iter().enumerate().any(|(i, tab)| {
                tab.index != i
                    || !tab.bounds.x.is_finite()
                    || !tab.bounds.width.is_finite()
                    || tab.bounds.width <= 0.
            })
        {
            return None;
        }
        let bounds = tabs.get(source)?.bounds;
        let switches: Vec<_> = tabs
            .iter()
            .filter(|tab| tab.index != source)
            .map(|tab| {
                let edge = bounds.x + if tab.index > source { bounds.width } else { 0. };
                tab.bounds.x + tab.bounds.width * 0.5 - edge
            })
            .collect();
        if switches.iter().any(|x| !x.is_finite()) || switches.windows(2).any(|p| p[0] >= p[1]) {
            return None;
        }
        Some(Self {
            group,
            source,
            press: press[0],
            tabs,
            clip,
            switches,
        })
    }

    pub fn grab_offset_x(&self) -> f32 {
        self.press - self.tabs[self.source].bounds.x
    }

    pub fn preview(&self, position: [f32; 2]) -> Option<TabDragPreview> {
        let delta = position[0] - self.press;
        if !delta.is_finite() {
            return None;
        }
        // A single immutable partition handles movement in either direction.
        // Animated rectangles and earlier pointer events never affect the slot.
        let slot = self.switches.partition_point(|point| *point <= delta);
        let bounds = self.tabs[self.source].bounds;
        let offsets = self
            .tabs
            .iter()
            .map(|tab| TabDragOffset {
                index: tab.index,
                x: if tab.index < self.source && tab.index >= slot {
                    bounds.width
                } else if tab.index > self.source && tab.index <= slot {
                    -bounds.width
                } else {
                    0.
                },
            })
            .collect();
        Some(TabDragPreview {
            bounds: Bounds {
                x: (bounds.x + delta).clamp(
                    self.clip.x,
                    (self.clip.x + self.clip.width - bounds.width).max(self.clip.x),
                ),
                ..bounds
            },
            // DockTarget uses an insertion index before removing the source.
            insertion: if slot > self.source { slot + 1 } else { slot },
            offsets,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs() -> Vec<TabHit> {
        [40., 100., 60., 80.]
            .into_iter()
            .enumerate()
            .scan(10., |x, (index, width)| {
                let tab = TabHit {
                    group: 7,
                    index,
                    bounds: Bounds {
                        x: *x,
                        y: 20.,
                        width,
                        height: 36.,
                    },
                };
                *x += width;
                Some(tab)
            })
            .collect()
    }

    #[test]
    fn frozen_variable_width_switches_are_early_and_history_independent() {
        let mut tabs = tabs();
        let clip = Bounds {
            x: 10.,
            y: 20.,
            width: 280.,
            height: 36.,
        };
        // Grab near either edge or in the center: the same drag distance swaps.
        for press in [51., 100., 149.] {
            let drag = TabDrag::new(7, 1, [press, 38.], &tabs, clip).unwrap();
            assert_eq!(drag.switches, [-20., 30., 100.]);
            for (delta, insertion, offsets) in [
                (0., 1, [0., 0., 0., 0.]),
                (29., 1, [0., 0., 0., 0.]),
                (30., 3, [0., 0., -100., 0.]),
                (30., 3, [0., 0., -100., 0.]),
                (29., 1, [0., 0., 0., 0.]),
                (100., 4, [0., 0., -100., -100.]),
                (-21., 0, [100., 0., 0., 0.]),
                (-20., 1, [0., 0., 0., 0.]),
            ] {
                let p = drag.preview([press + delta, 99.]).unwrap();
                assert_eq!(p.insertion, insertion);
                assert_eq!(p.offsets.iter().map(|o| o.x).collect::<Vec<_>>(), offsets);
                assert_eq!(p.bounds.y, 20.);
            }
        }
        // Input order does not matter, and later host measurements cannot move
        // a switch point. Clipping only limits the visual, not natural widths.
        tabs.reverse();
        let clip = Bounds {
            x: 80.,
            width: 50.,
            ..clip
        };
        let drag = TabDrag::new(7, 1, [100., 38.], &tabs, clip).unwrap();
        tabs[1].bounds.width = 1000.;
        let p = drag.preview([130., 38.]).unwrap();
        assert_eq!(p.insertion, 3);
        assert_eq!(p.bounds.x, 80.);
        assert_eq!(p.bounds.width, 100.);
        assert_eq!(p.offsets[2].x, -100.);
        assert!(drag.preview([f32::NAN, 38.]).is_none());
    }
}
