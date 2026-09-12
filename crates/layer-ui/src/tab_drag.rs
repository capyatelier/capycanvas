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

impl TabDragPreview {
    pub(crate) fn new(
        group: u32,
        source: usize,
        press: [f32; 2],
        position: [f32; 2],
        tabs: &[TabHit],
        clip: Bounds,
    ) -> Option<Self> {
        let tabs: Vec<_> = tabs.iter().filter(|tab| tab.group == group).collect();
        let bounds = tabs.iter().find(|tab| tab.index == source)?.bounds;
        if ![
            press[0],
            position[0],
            clip.x,
            clip.width,
            bounds.x,
            bounds.width,
        ]
        .into_iter()
        .all(f32::is_finite)
            || clip.width <= 0.
            || bounds.width <= 0.
        {
            return None;
        }
        let insertion = tabs
            .iter()
            .filter_map(|tab| {
                let visible = tab.bounds.intersection(clip)?;
                (position[0] < visible.x + visible.width * 0.5).then_some(tab.index)
            })
            .min()
            .unwrap_or_else(|| tabs.iter().map(|tab| tab.index + 1).max().unwrap_or(0));
        let offsets = tabs
            .iter()
            .map(|tab| TabDragOffset {
                index: tab.index,
                x: if tab.index < source && tab.index >= insertion {
                    bounds.width
                } else if tab.index > source && tab.index < insertion {
                    -bounds.width
                } else {
                    0.
                },
            })
            .collect();
        Some(Self {
            bounds: Bounds {
                x: (bounds.x + position[0] - press[0])
                    .clamp(clip.x, (clip.x + clip.width - bounds.width).max(clip.x)),
                ..bounds
            },
            insertion,
            offsets,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unequal_tabs_slide_across_fixed_midpoints_and_reverse() {
        let tabs = [40., 100., 60.]
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
            .collect::<Vec<_>>();
        let clip = Bounds {
            x: 10.,
            y: 20.,
            width: 240.,
            height: 36.,
        };
        for (x, insertion, offsets) in [
            (100., 2, [0., 0., 0.]),
            (181., 3, [0., 0., -100.]),
            (179., 2, [0., 0., 0.]),
            (29., 0, [100., 0., 0.]),
            (31., 1, [0., 0., 0.]),
        ] {
            let preview = TabDragPreview::new(7, 1, [100., 38.], [x, 99.], &tabs, clip).unwrap();
            assert_eq!(preview.insertion, insertion);
            assert_eq!(
                preview.offsets.iter().map(|o| o.x).collect::<Vec<_>>(),
                offsets
            );
            assert_eq!(preview.bounds.y, 20.);
        }
        for (x, expected) in [(-40., 10.), (300., 150.)] {
            let p = TabDragPreview::new(7, 1, [100., 38.], [x, 38.], &tabs, clip).unwrap();
            assert_eq!(p.bounds.x, expected);
        }
        // An oversized or partly scrolled tab retains its width and is clipped
        // by the host. Its natural width also determines neighbor displacement.
        let clip = Bounds {
            x: 80.,
            width: 50.,
            ..clip
        };
        let p = TabDragPreview::new(7, 1, [100., 38.], [200., 38.], &tabs, clip).unwrap();
        assert_eq!(p.bounds.x, 80.);
        assert_eq!(p.bounds.width, 100.);
        assert_eq!(p.offsets[2].x, -100.);
        let clip = Bounds {
            width: 100.,
            ..clip
        };
        let p = TabDragPreview::new(7, 1, [100., 38.], [166., 38.], &tabs, clip).unwrap();
        assert_eq!(
            p.insertion, 3,
            "A partly visible neighbor uses the same clipped midpoint as the drop target"
        );
        assert!(TabDragPreview::new(7, 1, [100., 38.], [f32::NAN, 38.], &tabs, clip).is_none());
    }
}
