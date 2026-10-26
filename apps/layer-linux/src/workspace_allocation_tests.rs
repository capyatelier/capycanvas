use super::{native_allocation, Axis, Bounds};

fn split(axis: Axis, boundary: f32) -> [Bounds; 3] {
    let widths = [boundary - 48., 6., 1000. - boundary - 6.];
    let starts = [48., boundary, boundary + 6.];
    std::array::from_fn(|i| match axis {
        Axis::Vertical => Bounds { x: 48., y: starts[i], width: 320., height: widths[i] },
        Axis::Horizontal => Bounds { x: starts[i], y: 48., width: widths[i], height: 320. },
    })
}

fn edges(bounds: Bounds, axis: Axis) -> [f32; 2] {
    match axis {
        Axis::Vertical => [bounds.y, bounds.y + bounds.height],
        Axis::Horizontal => [bounds.x, bounds.x + bounds.width],
    }
}

#[test]
fn adjacent_allocations_keep_fixed_gap_and_outer_edges() {
    let mut failures = Vec::new();
    for axis in [Axis::Vertical, Axis::Horizontal] {
        for step in 0..=32 {
            let boundary = 208. + step as f32 * 0.125;
            let [upper, divider, lower] = split(axis, boundary)
                .map(|b| edges(native_allocation(b), axis));
            let gap = lower[0] - upper[1];
            if (gap - 6.).abs() > 0.001 || (upper[1] - divider[0]).abs() > 0.001
                || (divider[1] - lower[0]).abs() > 0.001
                || (upper[0] - 48.).abs() > 0.001 || (lower[1] - 1000.).abs() > 0.001
            {
                failures.push(format!("{axis:?} boundary={boundary}: upper={upper:?}, divider={divider:?}, lower={lower:?}; expected gap=6 and outer=48..1000, actual gap={gap}"));
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn adjacent_allocations_resize_siblings_by_equal_opposite_steps() {
    let mut failures = Vec::new();
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let mut previous: Option<([f32; 2], f32)> = None;
        for step in 0..=32 {
            let boundary = 208. + step as f32 * 0.125;
            let [upper, _, lower] = split(axis, boundary)
                .map(|b| edges(native_allocation(b), axis));
            let sizes = [upper[1] - upper[0], lower[1] - lower[0]];
            if let Some((before, lower_start)) = previous {
                let growth = sizes[0] - before[0];
                let shrink = sizes[1] - before[1];
                let translation = lower[0] - lower_start;
                if (growth + shrink).abs() > 0.001 || (growth - translation).abs() > 0.001 {
                    failures.push(format!("{axis:?} boundary={boundary}: sibling size changes={growth}/{shrink}, lower origin change={translation}; expected equal/opposite sizes synchronized with the shared boundary"));
                    break;
                }
            }
            previous = Some((sizes, lower[0]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn resolved_split_allocations_preserve_shared_divider_and_parent_edges() {
    use layer_ui::{DockLayout, DockNode};
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let mut layout = DockLayout::default();
        layout.bands[0].extent = 800.;
        let mut previous: Option<[f32; 2]> = None;
        for step in 0..=64 {
            let DockNode::Split { axis: split_axis, fraction, .. } = &mut layout.bands[0].root else { unreachable!() };
            *split_axis = axis;
            *fraction = 0.25 + step as f32 / 128.;
            let resolved = layout.workspace(1800., 1100., 48., 0.);
            let divider = resolved.dividers.iter().find(|d| d.id == 4).unwrap();
            let upper = resolved.groups.iter().find(|g| g.id == 5).unwrap();
            let lower = resolved.groups.iter().find(|g| g.id == 6).unwrap();
            let [upper, divider, lower, parent] = [upper.bounds, divider.bounds, lower.bounds, divider.parent]
                .map(|b| edges(native_allocation(b), axis));
            assert_eq!(upper[0], parent[0]);
            assert_eq!(lower[1], parent[1]);
            assert_eq!(upper[1], divider[0]);
            assert_eq!(divider[1], lower[0]);
            assert_eq!(lower[0] - upper[1], 6.);
            let sizes = [upper[1] - upper[0], lower[1] - lower[0]];
            if let Some(before) = previous {
                assert_eq!(sizes[0] - before[0], -(sizes[1] - before[1]));
            }
            previous = Some(sizes);
        }
    }
}
