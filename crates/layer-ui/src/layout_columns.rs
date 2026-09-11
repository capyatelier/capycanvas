//! Collapsing is a projection of the existing recursive dock tree. Its nodes,
//! tab selection and internal split ratios survive; no floating panels or
//! duplicate panel registrations are created.
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollapsedColumn {
    pub root: u32,
    pub expanded_width: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnIcon {
    pub panel: Panel,
    pub bounds: Bounds,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollapsedGroup {
    pub group: u32,
    pub active: Panel,
    pub bounds: Bounds,
    pub icons: Vec<ColumnIcon>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollapsedColumnPlacement {
    pub id: u32,
    pub bounds: Bounds,
    pub expand: Bounds,
    pub grip: Bounds,
    pub empty: Bounds,
    /// Clip/scroll the groups in this area, leaving expand and grip fixed.
    pub content: Bounds,
    pub groups: Vec<CollapsedGroup>,
}
impl CollapsedColumnPlacement {
    pub fn expand_label(&self) -> &'static str {
        "Expand column"
    }
    pub fn expand_action(&self) -> crate::UiAction {
        crate::UiAction::Customize {
            action: crate::CustomizationAction::SetColumnCollapsed {
                group: self.id,
                collapsed: false,
            },
        }
    }
    pub(super) fn scroll(&mut self, offset: f32) {
        let bottom = self
            .groups
            .last()
            .map_or(self.content.y, |g| g.bounds.y + g.bounds.height);
        let offset = offset.clamp(0., (bottom - self.content.y - self.content.height).max(0.));
        for g in &mut self.groups {
            g.bounds.y -= offset;
            for i in &mut g.icons {
                i.bounds.y -= offset;
            }
        }
        self.empty.y = (bottom - offset + WORKSPACE_SPACING).min(self.grip.y);
        self.empty.height = (self.grip.y - self.empty.y).max(0.);
    }
    /// Vertical strips insert tabs vertically; inter-group gaps create groups.
    /// Expand/grip controls and clipped overflow are never accidental targets.
    pub fn drop_hint(&self, point: [f32; 2]) -> Option<DropHint> {
        let [x, y] = point;
        if !self.bounds.contains(x, y)
            || self.expand.contains(x, y)
            || self.grip.contains(x, y)
            || (!self.content.contains(x, y) && !self.empty.contains(x, y))
        {
            return None;
        }
        for group in &self.groups {
            if group
                .bounds
                .intersection(self.content)
                .is_some_and(|b| b.contains(x, y))
            {
                let index = group
                    .icons
                    .iter()
                    .position(|i| y < i.bounds.y + i.bounds.height * 0.5)
                    .unwrap_or(group.icons.len());
                let at = group
                    .icons
                    .get(index)
                    .map_or(group.bounds.y + group.bounds.height, |i| i.bounds.y);
                return Some(DropHint {
                    target: DockTarget::Tab {
                        group: group.group,
                        index: Some(index),
                    },
                    bounds: Bounds {
                        x: group.bounds.x,
                        y: (at - 1.5).clamp(
                            self.content.y,
                            (self.content.y + self.content.height - 3.).max(self.content.y),
                        ),
                        width: group.bounds.width,
                        height: 3.0_f32.min(self.content.height),
                    },
                });
            }
            if y < group.bounds.y {
                return Some(DropHint {
                    target: DockTarget::Split {
                        group: group.group,
                        edge: Edge::Top,
                    },
                    bounds: edge_line(group.bounds, Edge::Top),
                });
            }
        }
        let last = self.groups.last()?;
        Some(DropHint {
            target: DockTarget::Split {
                group: last.group,
                edge: Edge::Bottom,
            },
            bounds: Bounds {
                y: self.empty.y,
                height: 3.0_f32.min(self.empty.height),
                ..self.empty
            },
        })
    }
    pub(super) fn translate(&mut self, delta: [f32; 2]) {
        let shift = |b: &mut Bounds| {
            b.x += delta[0];
            b.y += delta[1];
        };
        shift(&mut self.bounds);
        shift(&mut self.expand);
        shift(&mut self.grip);
        shift(&mut self.empty);
        shift(&mut self.content);
        for group in &mut self.groups {
            shift(&mut group.bounds);
            for icon in &mut group.icons {
                shift(&mut icon.bounds);
            }
        }
    }
}

impl DockLayout {
    pub fn is_collapsed(&self, root: u32) -> bool {
        self.collapsed.iter().any(|c| c.root == root)
    }

    /// The closest horizontal split establishes a separate column; vertical
    /// splits stack groups within it. Standalone toolbars retain ribbon behavior.
    pub fn column_for_group(&self, group: u32) -> Option<u32> {
        fn find(node: &DockNode, group: u32, column: u32) -> Option<u32> {
            if node.id() == group {
                return Some(column);
            }
            let DockNode::Split {
                axis,
                first,
                second,
                ..
            } = node
            else {
                return None;
            };
            [first, second].into_iter().find_map(|child| {
                find(
                    child,
                    group,
                    if *axis == Axis::Horizontal {
                        child.id()
                    } else {
                        column
                    },
                )
            })
        }
        self.group_panels(group).ok()?;
        let band = self.bands.iter().find(|b| b.root.find(group).is_some())?;
        if !matches!(band.edge, Edge::Left | Edge::Right) {
            return None;
        }
        let root = find(&band.root, group, band.root.id())?;
        if matches!(band.root.find(root)?, DockNode::Tabs { panels, active, .. }
            if panels.len() == 1 && active.kind() == PanelKind::Tiles)
        {
            return None;
        }
        Some(root)
    }

    pub fn collapsed_column_for_group(&self, group: u32) -> Option<u32> {
        fn visible(node: &DockNode, group: u32, layout: &DockLayout) -> Option<u32> {
            node.find(group)?;
            if layout.is_collapsed(node.id()) {
                return Some(node.id());
            }
            match node {
                DockNode::Tabs { .. } => None,
                DockNode::Split { first, second, .. } => {
                    visible(first, group, layout).or_else(|| visible(second, group, layout))
                }
            }
        }
        // An expanded column may itself contain collapsed subcolumns. When
        // its parent collapses too, only the outer strip is a visible anchor.
        self.bands
            .iter()
            .find_map(|b| visible(&b.root, group, self))
    }

    /// A column moves intact through the same dock tree, never through a float
    /// or a tab merge. Detach first so the existing width-reclamation rules also
    /// apply to nested source columns.
    pub(super) fn move_column(
        &mut self,
        viewport: [f32; 2],
        column: u32,
        target: DockTarget,
    ) -> Result<(), String> {
        if !self.is_collapsed(column) {
            return Err("Only collapsed columns move as a unit".into());
        }
        let moving = self.node(column).ok_or("Unknown column")?.clone();
        let before = self.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        match target {
            DockTarget::Edge {
                edge: Edge::Left | Edge::Right,
                ..
            } => {}
            DockTarget::BesideBand { band }
                if self
                    .bands
                    .iter()
                    .any(|b| b.id == band && matches!(b.edge, Edge::Left | Edge::Right)) =>
            {
                if self
                    .bands
                    .iter()
                    .any(|b| b.id == band && b.root.id() == column)
                {
                    return Ok(());
                }
            }
            DockTarget::Split {
                group,
                edge: Edge::Left | Edge::Right,
            } if matches!(self.group_edge(group), Some(Edge::Left | Edge::Right)) => {
                if moving.find(group).is_some() {
                    return Ok(());
                }
            }
            _ => return Err("Columns dock at side edges or beside other columns".into()),
        }
        let panels: Vec<_> = self
            .panels
            .iter()
            .filter_map(|p| moving.group_for(p.id).map(|_| p.id))
            .collect();
        let collapsed: Vec<_> = self
            .collapsed
            .iter()
            .filter(|c| moving.find(c.root).is_some())
            .cloned()
            .collect();
        let mut next = self.clone();
        next.detach(&panels);
        next.reclaim_removed_columns(self, &before);
        match target {
            DockTarget::Edge { .. } | DockTarget::BesideBand { .. } => {
                let (edge, index) = match target {
                    DockTarget::Edge { edge, outer } => {
                        (edge, if outer { 0 } else { next.bands.len() })
                    }
                    DockTarget::BesideBand { band } => {
                        let index = next
                            .bands
                            .iter()
                            .position(|b| b.id == band)
                            .ok_or("The target column no longer exists")?;
                        (next.bands[index].edge, index + 1)
                    }
                    _ => unreachable!(),
                };
                let id = next.allocate()?;
                next.bands.insert(
                    index,
                    DockBand {
                        id,
                        edge,
                        extent: TILE_SIZE + WORKSPACE_SPACING,
                        root: moving,
                    },
                );
            }
            DockTarget::Split { group, edge } => {
                let after = next.workspace(
                    viewport[0],
                    viewport[1],
                    crate::HEADER_HEIGHT,
                    crate::STATUS_HEIGHT,
                );
                let width = subtree_bounds(
                    next.node(group)
                        .ok_or("The target column no longer exists")?,
                    &after,
                )
                .ok_or("The target column is not visible")?
                .width;
                let band = next
                    .bands
                    .iter_mut()
                    .find(|b| b.root.find(group).is_some())
                    .ok_or("The target column is not docked")?;
                band.extent = column_width(
                    &mut band.root,
                    group,
                    width + TILE_SIZE + WORKSPACE_SPACING,
                    &after,
                )
                .ok_or("Invalid target column")?
                    + WORKSPACE_SPACING;
                let id = next.allocate()?;
                let node = next.node_mut(group).unwrap();
                let (first, second, fraction) = if edge == Edge::Left {
                    (moving, node.clone(), TILE_SIZE / (width + TILE_SIZE))
                } else {
                    (node.clone(), moving, width / (width + TILE_SIZE))
                };
                *node = DockNode::Split {
                    id,
                    axis: Axis::Horizontal,
                    fraction,
                    first: Box::new(first),
                    second: Box::new(second),
                };
            }
            _ => unreachable!(),
        }
        next.collapsed.extend(collapsed);
        if self.same_placement(&next) {
            return Ok(());
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub(crate) fn column_drop_hint(
        &self,
        resolved: &ResolvedLayout,
        source: u32,
        point: [f32; 2],
    ) -> Option<DropHint> {
        let moving = self.node(source)?;
        // Only the visible outside of a column is eligible, not its internal
        // stacked groups. The closest boundary wins in gaps between columns.
        let mut roots: Vec<_> = resolved.collapsed.iter().map(|c| c.id).collect();
        roots.extend(
            resolved
                .groups
                .iter()
                .filter(|g| !g.floating)
                .filter_map(|g| {
                    self.column_for_group(g.id).or_else(|| {
                        matches!(self.group_edge(g.id), Some(Edge::Left | Edge::Right))
                            .then_some(g.id)
                    })
                }),
        );
        roots.sort_unstable();
        roots.dedup();
        let mut nearest = None;
        let mut distance = f32::INFINITY;
        for root in roots {
            if moving.find(root).is_some() {
                continue;
            }
            let b = subtree_bounds(self.node(root)?, resolved)?;
            if point[1] < b.y || point[1] > b.y + b.height {
                continue;
            }
            for (edge, x) in [(Edge::Left, b.x), (Edge::Right, b.x + b.width)] {
                let d = (point[0] - x).abs();
                if d <= WORKSPACE_PROXIMITY && d < distance {
                    distance = d;
                    nearest = Some(DropHint {
                        target: DockTarget::Split { group: root, edge },
                        bounds: edge_line(b, edge),
                    });
                }
            }
        }
        if nearest.is_some() {
            return nearest;
        }
        resolved
            .drop_hint(point[0], point[1], &[], true)
            .filter(|h| {
                matches!(
                    h.target,
                    DockTarget::Edge {
                        edge: Edge::Left | Edge::Right,
                        ..
                    } | DockTarget::BesideBand { .. }
                )
            })
    }

    /// `group` is any contained tab group when collapsing, or the strip's root
    /// when expanding. Only width outside the subtree changes; its internal
    /// split proportions remain the expanded layout's proportions.
    pub fn set_column_collapsed(
        &mut self,
        group: u32,
        collapsed: bool,
        viewport: [f32; 2],
    ) -> Result<(), String> {
        if !viewport.into_iter().all(|v| v.is_finite() && v > 0.) {
            return Err("Invalid workspace size".into());
        }
        let root = if self.is_collapsed(group)
            || matches!(self.node(group), Some(DockNode::Split { .. }))
        {
            group
        } else {
            self.column_for_group(group)
                .ok_or("This group has no collapsible column")?
        };
        if self.is_collapsed(root) == collapsed {
            return Ok(());
        }
        if !self
            .bands
            .iter()
            .any(|b| matches!(b.edge, Edge::Left | Edge::Right) && b.root.find(root).is_some())
        {
            return Err("This group has no collapsible column".into());
        }
        let before = self.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        let node = self.node(root).ok_or("The column no longer exists")?;
        let bounds = subtree_bounds(node, &before).ok_or("The column is not visible")?;
        let width = if collapsed {
            self.collapsed.push(CollapsedColumn {
                root,
                expanded_width: bounds.width,
            });
            TILE_SIZE
        } else {
            let index = self.collapsed.iter().position(|c| c.root == root).unwrap();
            self.collapsed.remove(index).expanded_width
        };
        let band = self
            .bands
            .iter_mut()
            .find(|b| b.root.find(root).is_some())
            .unwrap();
        band.extent =
            column_width(&mut band.root, root, width, &before).unwrap() + WORKSPACE_SPACING;
        Ok(())
    }
    pub(crate) fn column_width_before_resize(&self, root: u32, viewport: [f32; 2]) -> Option<f32> {
        let geometry = self.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        subtree_bounds(self.node(root)?, &geometry).map(|b| b.width)
    }

    /// Collapsed columns touching the left and right sides of a width divider.
    /// Follow horizontal splits so a band's outside handle can also open its
    /// outermost collapsed subcolumn. A partially collapsed vertical stack
    /// still resizes as a whole through its ordinary band handle.
    pub(crate) fn collapsed_divider_columns(&self, divider: &Divider) -> [Option<u32>; 2] {
        fn edge_column(layout: &DockLayout, node: &DockNode, right: bool) -> Option<u32> {
            if layout.is_collapsed(node.id()) {
                return Some(node.id());
            }
            if let DockNode::Split {
                axis: Axis::Horizontal,
                first,
                second,
                ..
            } = node
            {
                edge_column(layout, if right { second } else { first }, right)
            } else {
                None
            }
        }
        if divider.axis != Axis::Horizontal {
            return [None; 2];
        }
        if divider.band {
            let Some(band) = self.bands.iter().find(|b| b.id == divider.id) else {
                return [None; 2];
            };
            let mut columns = [None; 2];
            columns[usize::from(divider.reversed)] =
                edge_column(self, &band.root, !divider.reversed);
            columns
        } else if let Some(DockNode::Split { first, second, .. }) = self.node(divider.id) {
            [
                edge_column(self, first, true),
                edge_column(self, second, false),
            ]
        } else {
            [None; 2]
        }
    }

    pub(crate) fn collapse_at_divider(
        &self,
        id: u32,
        position: [f32; 2],
        viewport: [f32; 2],
    ) -> Option<u32> {
        let geometry = self.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        let divider = geometry
            .dividers
            .iter()
            .find(|d| d.id == id && d.axis == Axis::Horizontal)?;
        let left = position[0] - divider.parent.x - WORKSPACE_SPACING * 0.5;
        let right = divider.parent.x + divider.parent.width - position[0] - WORKSPACE_SPACING * 0.5;
        let should_collapse = |node: &DockNode, width: f32| {
            if self.is_collapsed(node.id()) {
                return false;
            }
            let minimum = tab_min_width(node, self).max(ribbon_cross_min(
                node,
                Axis::Vertical,
                divider.parent.height,
                self,
            ));
            // Use the unclamped drag width: collapse more than 25% into the
            // allocation minimum, or at the existing icon-strip threshold.
            width < minimum * 0.75 || width <= TILE_SIZE
        };
        let root = if divider.band {
            let node = &self.bands.iter().find(|b| b.id == id)?.root;
            if !should_collapse(node, if divider.reversed { right } else { left }) {
                return None;
            }
            node.id()
        } else {
            let DockNode::Split { first, second, .. } = self.node(id)? else {
                return None;
            };
            if should_collapse(first, left) {
                first.id()
            } else if should_collapse(second, right) {
                second.id()
            } else {
                return None;
            }
        };
        if matches!(self.node(root)?, DockNode::Tabs { panels, active, .. } if panels.len() == 1 && active.kind() == PanelKind::Tiles)
        {
            return None;
        }
        Some(root)
    }

    pub(super) fn validate_columns(&self) -> Result<(), String> {
        for (index, column) in self.collapsed.iter().enumerate() {
            if !column.expanded_width.is_finite()
                || column.expanded_width <= 0.
                || self.collapsed[..index]
                    .iter()
                    .any(|c| c.root == column.root)
                || !self.bands.iter().any(|b| {
                    matches!(b.edge, Edge::Left | Edge::Right) && b.root.find(column.root).is_some()
                })
            {
                return Err("Invalid collapsed column".into());
            }
        }
        Ok(())
    }

    // Removing the final member of one branch can replace a split root with
    // its surviving child. Transfer collapse state to that child, not a stale ID.
    pub(super) fn detach_column_members(&mut self, panels: &[Panel]) {
        let mut columns: Vec<CollapsedColumn> = Vec::new();
        for column in &self.collapsed {
            let mut retained = self.node(column.root).cloned();
            for panel in panels {
                retained = retained.and_then(|n| n.remove(*panel));
            }
            if let Some(node) = retained {
                if let Some(existing) = columns.iter_mut().find(|c| c.root == node.id()) {
                    existing.expanded_width = existing.expanded_width.max(column.expanded_width);
                } else {
                    columns.push(CollapsedColumn {
                        root: node.id(),
                        ..column.clone()
                    });
                }
            }
        }
        self.collapsed = columns;
    }
}

fn subtree_bounds(node: &DockNode, geometry: &ResolvedLayout) -> Option<Bounds> {
    if let Some(column) = geometry.collapsed.iter().find(|c| c.id == node.id()) {
        return Some(column.bounds);
    }
    match node {
        DockNode::Tabs { id, .. } => geometry
            .groups
            .iter()
            .find(|g| g.id == *id)
            .map(|g| g.bounds),
        DockNode::Split { first, second, .. } => {
            let a = subtree_bounds(first, geometry)?;
            let b = subtree_bounds(second, geometry)?;
            Some(Bounds {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
                width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
                height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
            })
        }
    }
}
// Size the changed branch from actual child allocations, not saved fractions.
// Full-width rows follow it; independently split rows retain their own widths.
fn column_width(
    node: &mut DockNode,
    root: u32,
    width: f32,
    geometry: &ResolvedLayout,
) -> Option<f32> {
    if node.id() == root {
        return Some(width);
    }
    if let DockNode::Split {
        axis,
        fraction,
        first,
        second,
        ..
    } = node
    {
        let in_first = first.find(root).is_some();
        let (changed, other) = if in_first {
            (first, second)
        } else {
            (second, first)
        };
        let changed_width = column_width(changed, root, width, geometry)?;
        let other_width = subtree_bounds(other, geometry)?.width;
        if *axis == Axis::Horizontal {
            let total = changed_width + other_width;
            *fraction = if in_first { changed_width } else { other_width } / total.max(1.);
            Some(total + WORKSPACE_SPACING)
        } else {
            fn split_columns(node: &DockNode) -> bool {
                match node {
                    DockNode::Tabs { .. } => false,
                    DockNode::Split {
                        axis,
                        first,
                        second,
                        ..
                    } => *axis == Axis::Horizontal || split_columns(first) || split_columns(second),
                }
            }
            Some(changed_width.max(if split_columns(other) {
                other_width
            } else {
                0.
            }))
        }
    } else {
        None
    }
}

pub(super) fn resolve_column(node: &DockNode, bounds: Bounds) -> CollapsedColumnPlacement {
    let width = bounds.width.min(TILE_SIZE);
    let bounds = Bounds { width, ..bounds };
    let expand = Bounds {
        height: 24.0_f32.min(bounds.height * 0.5),
        width,
        ..bounds
    };
    let grip_height = PANEL_GRIP_HEIGHT.min(bounds.height - expand.height);
    let grip = Bounds {
        y: bounds.y + bounds.height - grip_height,
        height: grip_height,
        width,
        ..bounds
    };
    let top = (expand.y + expand.height + WORKSPACE_SPACING).min(grip.y);
    let content = Bounds {
        y: top,
        height: (grip.y - WORKSPACE_SPACING - top).max(0.),
        width,
        ..bounds
    };
    let mut groups = Vec::new();
    fn visit(node: &DockNode, content: Bounds, y: &mut f32, groups: &mut Vec<CollapsedGroup>) {
        match node {
            DockNode::Tabs {
                id, panels, active, ..
            } => {
                let bounds = Bounds {
                    y: *y,
                    height: panels.len() as f32 * (TILE_SIZE + 2.) - 2.,
                    ..content
                };
                let icons = panels
                    .iter()
                    .enumerate()
                    .map(|(index, panel)| ColumnIcon {
                        panel: *panel,
                        bounds: Bounds {
                            y: *y + index as f32 * (TILE_SIZE + 2.),
                            height: TILE_SIZE,
                            ..content
                        },
                    })
                    .collect();
                groups.push(CollapsedGroup {
                    group: *id,
                    active: *active,
                    bounds,
                    icons,
                });
                *y += bounds.height + WORKSPACE_SPACING;
            }
            DockNode::Split { first, second, .. } => {
                visit(first, content, y, groups);
                visit(second, content, y, groups);
            }
        }
    }
    let mut y = content.y;
    visit(node, content, &mut y, &mut groups);
    let empty = Bounds {
        y: y.min(grip.y),
        height: (grip.y - y).max(0.),
        width,
        ..bounds
    };
    CollapsedColumnPlacement {
        id: node.id(),
        bounds,
        expand,
        grip,
        content,
        empty,
        groups,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VIEW: [f32; 2] = [1600., 1000.];
    fn geometry(layout: &DockLayout) -> ResolvedLayout {
        layout.workspace(VIEW[0], VIEW[1], crate::HEADER_HEIGHT, crate::STATUS_HEIGHT)
    }

    fn assert_collapse_threshold(
        layout: &DockLayout,
        divider: u32,
        root: u32,
        right: bool,
        threshold: f32,
        inclusive: bool,
    ) {
        let resolved = geometry(layout);
        let d = resolved.dividers.iter().find(|d| d.id == divider).unwrap();
        for (width, expected) in [
            (threshold + 0.5, None),
            (threshold, inclusive.then_some(root)),
            (threshold - 0.5, Some(root)),
        ] {
            let x = if right {
                d.parent.x + d.parent.width - width - WORKSPACE_SPACING * 0.5
            } else {
                d.parent.x + width + WORKSPACE_SPACING * 0.5
            };
            assert_eq!(
                layout.collapse_at_divider(divider, [x, d.bounds.y + 20.], VIEW),
                expected,
                "divider {divider}, root {root}, requested width {width}"
            );
        }
    }

    #[test]
    fn resize_collapse_uses_minimum_width_on_both_edges() {
        for layout in [DockLayout::default(), DockLayout::editor_default()] {
            for (panel, minimum) in [
                (Panel::Brushes, TOOL_PANEL_MIN_WIDTH),
                (Panel::Layers, LAYERS_MIN_WIDTH),
            ] {
                let band = layout
                    .bands
                    .iter()
                    .find(|b| b.root.group_for(panel).is_some())
                    .unwrap();
                assert_collapse_threshold(
                    &layout,
                    band.id,
                    band.root.id(),
                    band.edge == Edge::Right,
                    minimum * 0.75,
                    false,
                );
            }
        }
    }

    #[test]
    fn resize_collapse_uses_each_nested_column_and_preserves_old_threshold() {
        let mut layout = DockLayout::default();
        let DockNode::Split { axis, .. } = &mut layout.bands[0].root else {
            unreachable!();
        };
        *axis = Axis::Horizontal;
        assert_collapse_threshold(&layout, 4, 5, false, TOOL_PANEL_MIN_WIDTH * 0.75, false);
        // Brush size has no content minimum: the existing icon-width trigger
        // still wins for this narrow column.
        assert_collapse_threshold(&layout, 4, 6, true, TILE_SIZE, true);

        // The outer divider uses the combined minimum of its two columns.
        assert_collapse_threshold(
            &layout,
            3,
            4,
            false,
            (TOOL_PANEL_MIN_WIDTH + WORKSPACE_SPACING) * 0.75,
            false,
        );
    }

    #[test]
    fn resize_collapse_follows_measured_minimum_and_ignores_row_dividers() {
        let mut layout = DockLayout::default();
        layout.fit_tab_groups.push(5);
        layout.measurements.push(PanelMeasurement {
            panel: Panel::Brushes,
            tab_width: 300.,
            content_height: 0.,
        });
        assert_collapse_threshold(&layout, 3, 4, false, 320. * 0.75, false);
        assert_eq!(layout.collapse_at_divider(4, [0., 0.], VIEW), None);

        // Standalone tool ribbons retain their existing resize behavior.
        layout.bands[2].edge = Edge::Left;
        let d = geometry(&layout)
            .dividers
            .into_iter()
            .find(|d| d.id == 1)
            .unwrap();
        assert_eq!(
            layout.collapse_at_divider(1, [d.parent.x, d.bounds.y + 20.], VIEW),
            None
        );
    }

    #[test]
    fn collapsed_parent_owns_drawers_until_it_reveals_its_collapsed_child() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEW,
                Panel::Properties,
                DockTarget::Split {
                    group: 5,
                    edge: Edge::Right,
                },
            )
            .unwrap();
        let child = layout.panel_group(Panel::Properties).unwrap();
        layout.set_column_collapsed(child, true, VIEW).unwrap();
        layout.set_column_collapsed(6, true, VIEW).unwrap();
        assert_eq!(layout.collapsed.len(), 2);
        assert_eq!(geometry(&layout).collapsed.len(), 1);
        assert_eq!(layout.collapsed_column_for_group(child), Some(4));
        let drawer = crate::ContentDrawer::for_column(&layout, child, Panel::Properties).unwrap();
        assert!(drawer.placement(&layout, VIEW, &[200.], false).is_some());
        layout.set_column_collapsed(4, false, VIEW).unwrap();
        assert_eq!(layout.collapsed_column_for_group(child), Some(child));
        assert!(
            crate::ContentDrawer::for_column(&layout, child, Panel::Properties)
                .unwrap()
                .placement(&layout, VIEW, &[200.], false)
                .is_some()
        );
    }
    #[test]
    fn whole_column_moves_preserve_subtree_and_neighbor_widths() {
        for target in [
            DockTarget::Edge {
                edge: Edge::Right,
                outer: true,
            },
            DockTarget::BesideBand { band: 7 },
            DockTarget::Split {
                group: 8,
                edge: Edge::Left,
            },
            DockTarget::Split {
                group: 8,
                edge: Edge::Right,
            },
        ] {
            let mut layout = DockLayout::default();
            layout.set_column_collapsed(5, true, VIEW).unwrap();
            let moving = layout.node(4).unwrap().clone();
            let column = layout.collapsed[0].clone();
            let before = geometry(&layout);
            layout
                .move_item(VIEW, DockItem::Column { column: 4 }, target)
                .unwrap();
            assert_eq!(layout.node(4), Some(&moving));
            assert_eq!(layout.collapsed, [column]);
            assert_eq!(layout.group_edge(5), Some(Edge::Right));
            assert!(layout.floating.is_empty());
            let after = geometry(&layout);
            assert_eq!(after.collapsed[0].bounds.width, TILE_SIZE);
            assert!(
                (subtree_bounds(layout.node(8).unwrap(), &after)
                    .unwrap()
                    .width
                    - subtree_bounds(layout.node(8).unwrap(), &before)
                        .unwrap()
                        .width)
                    .abs()
                    < 0.5
            );
            layout.validate().unwrap();
            let saved = serde_json::to_string(&layout).unwrap();
            let restored: DockLayout = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored, layout);
        }
    }
    #[test]
    fn column_moves_reject_float_merge_and_preserve_same_slot() {
        let mut layout = DockLayout::default();
        layout.set_column_collapsed(5, true, VIEW).unwrap();
        let before = layout.clone();
        for target in [
            DockTarget::Float {
                position: [600., 400.],
            },
            DockTarget::Tab {
                group: 8,
                index: None,
            },
            DockTarget::Edge {
                edge: Edge::Top,
                outer: true,
            },
            DockTarget::Split {
                group: 8,
                edge: Edge::Bottom,
            },
        ] {
            assert!(
                layout
                    .move_item(VIEW, DockItem::Column { column: 4 }, target)
                    .is_err()
            );
            assert_eq!(layout, before);
        }
        layout
            .move_item(
                VIEW,
                DockItem::Column { column: 4 },
                DockTarget::Edge {
                    edge: Edge::Left,
                    outer: true,
                },
            )
            .unwrap();
        assert_eq!(layout, before);
        let resolved = geometry(&layout);
        assert!(
            layout
                .column_drop_hint(&resolved, 4, [800., 500.])
                .is_none()
        );
        let right = subtree_bounds(layout.node(8).unwrap(), &resolved).unwrap();
        let hint = layout
            .column_drop_hint(&resolved, 4, [right.x - 2., right.y + 180.])
            .unwrap();
        assert_eq!(
            hint.target,
            DockTarget::Split {
                group: 8,
                edge: Edge::Left
            }
        );
        layout
            .move_item(VIEW, DockItem::Column { column: 4 }, hint.target)
            .unwrap();
    }
    #[test]
    fn moving_nested_column_reclaims_only_its_source_width() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEW,
                Panel::Properties,
                DockTarget::Split {
                    group: 5,
                    edge: Edge::Right,
                },
            )
            .unwrap();
        let id = layout.panel_group(Panel::Properties).unwrap();
        layout.set_column_collapsed(id, true, VIEW).unwrap();
        let before = geometry(&layout);
        layout
            .move_item(
                VIEW,
                DockItem::Column { column: id },
                DockTarget::Edge {
                    edge: Edge::Right,
                    outer: true,
                },
            )
            .unwrap();
        let after = geometry(&layout);
        let width = |r: &ResolvedLayout, group| {
            r.groups
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds
                .width
        };
        assert!((width(&before, 5) - width(&after, 5)).abs() < 0.5);
        assert!((width(&before, 6) - width(&after, 6) - TILE_SIZE - WORKSPACE_SPACING).abs() < 0.5);
        assert!(layout.is_collapsed(id));
        layout.set_column_collapsed(id, false, VIEW).unwrap();
        layout.validate().unwrap();
    }
    #[test]
    fn collapse_preserves_groups_and_restores_exact_widths() {
        let mut layout = DockLayout::default();
        let before = layout.clone();
        let expanded = geometry(&layout);
        layout.set_column_collapsed(5, true, VIEW).unwrap();
        layout.validate().unwrap();
        assert_eq!(layout.bands[0].root, before.bands[0].root);
        let narrow = geometry(&layout);
        let col = &narrow.collapsed[0];
        assert_eq!(col.bounds.width, TILE_SIZE);
        assert_eq!(
            col.groups.iter().map(|g| g.group).collect::<Vec<_>>(),
            [5, 6]
        );
        assert_eq!(col.groups[0].icons[0].panel, Panel::Brushes);
        assert_eq!(
            col.grip.y + col.grip.height,
            col.bounds.y + col.bounds.height
        );
        assert!(narrow.work_area.width > expanded.work_area.width);
        assert!(!narrow.groups.iter().any(|g| g.id == 5 || g.id == 6));
        layout.set_column_collapsed(4, false, VIEW).unwrap();
        assert_eq!(layout, before);
        assert_eq!(
            geometry(&layout).groups[0].bounds,
            expanded.groups[0].bounds
        );
    }
    #[test]
    fn nested_columns_keep_neighbors_fixed_and_survive_removing_members() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEW,
                Panel::Properties,
                DockTarget::Split {
                    group: 5,
                    edge: Edge::Right,
                },
            )
            .unwrap();
        let group = layout.panel_group(Panel::Properties).unwrap();
        let width =
            |r: &ResolvedLayout, id| r.groups.iter().find(|g| g.id == id).unwrap().bounds.width;
        let before = geometry(&layout);
        assert_eq!(layout.column_for_group(group), Some(group));
        layout.set_column_collapsed(group, true, VIEW).unwrap();
        assert!((width(&geometry(&layout), 5) - width(&before, 5)).abs() < 0.01);
        layout.set_column_collapsed(group, false, VIEW).unwrap();
        assert!((width(&geometry(&layout), group) - width(&before, group)).abs() < 0.01);
        layout.set_column_collapsed(6, true, VIEW).unwrap();
        layout.set_panel_visible(Panel::Brushes, false).unwrap();
        layout.validate().unwrap();
        let saved = serde_json::to_string(&layout).unwrap();
        let loaded: DockLayout = serde_json::from_str(&saved).unwrap();
        loaded.validate().unwrap();
        assert_eq!(loaded.collapsed, layout.collapsed);
        assert_eq!(geometry(&loaded).collapsed.len(), 1);
        layout.set_panel_visible(Panel::Properties, false).unwrap();
        layout.validate().unwrap();
        assert_eq!(layout.collapsed[0].root, 6);
        layout.set_panel_visible(Panel::Sizes, false).unwrap();
        layout.validate().unwrap();
        assert!(layout.collapsed.is_empty());
    }
    #[test]
    fn invalid_columns_and_toolbar_columns_are_rejected() {
        let mut layout = DockLayout::default();
        assert!(layout.set_column_collapsed(2, true, VIEW).is_err());
        assert!(
            layout
                .set_column_collapsed(5, true, [f32::NAN, 100.])
                .is_err()
        );
        layout.collapsed.push(CollapsedColumn {
            root: 999,
            expanded_width: 200.,
        });
        assert!(layout.validate().is_err());
        layout.collapsed[0].root = 2;
        assert!(layout.validate().is_err());
    }

    #[test]
    fn collapsed_drop_targets_keep_tab_and_group_insertion_distinct() {
        let mut layout = DockLayout::default();
        layout.set_column_collapsed(5, true, VIEW).unwrap();
        let r = geometry(&layout);
        let c = &r.collapsed[0];
        let x = c.bounds.x + c.bounds.width * 0.5;
        let hint = r
            .drop_hint(x, c.groups[0].bounds.y + 2., &[], true)
            .unwrap();
        assert_eq!(
            hint.target,
            DockTarget::Tab {
                group: 5,
                index: Some(0)
            }
        );
        assert!(
            r.drop_hint(x, c.groups[0].bounds.y + 2., &[], false)
                .is_none()
        );
        layout
            .move_panel(VIEW, Panel::Properties, hint.target)
            .unwrap();
        assert_eq!(
            layout.group_panels(5).unwrap(),
            [Panel::Properties, Panel::Brushes]
        );
        assert_eq!(layout.collapsed[0].root, c.id);
        let r = geometry(&layout);
        let c = &r.collapsed[0];
        let hint = c.drop_hint([x, c.groups[1].bounds.y - 2.]).unwrap();
        assert_eq!(
            hint.target,
            DockTarget::Split {
                group: 6,
                edge: Edge::Top
            }
        );
        layout
            .move_panel(VIEW, Panel::Adjustments, hint.target)
            .unwrap();
        layout.validate().unwrap();
        let r = geometry(&layout);
        let c = &r.collapsed[0];
        assert_eq!(c.groups.len(), 3);
        assert_eq!(c.groups[1].icons[0].panel, Panel::Adjustments);
        let hint = c.drop_hint([x, c.empty.y + 2.]).unwrap();
        assert_eq!(
            hint.target,
            DockTarget::Split {
                group: 6,
                edge: Edge::Bottom
            }
        );
        assert!(c.drop_hint([x, c.expand.y + 2.]).is_none());
        assert!(c.drop_hint([x, c.grip.y + 2.]).is_none());
    }

    #[test]
    fn adding_a_group_to_a_single_group_column_preserves_collapse() {
        let mut layout = DockLayout::default();
        layout.set_column_collapsed(8, true, VIEW).unwrap();
        layout
            .move_panel(
                VIEW,
                Panel::Sizes,
                DockTarget::Split {
                    group: 8,
                    edge: Edge::Top,
                },
            )
            .unwrap();
        layout.validate().unwrap();
        let r = geometry(&layout);
        assert_eq!(r.collapsed.len(), 1);
        assert_eq!(r.collapsed[0].groups.len(), 2);
        assert_eq!(r.collapsed[0].groups[0].icons[0].panel, Panel::Sizes);
        assert_eq!(r.collapsed[0].bounds.width, TILE_SIZE);
    }

    #[test]
    fn collapse_does_not_reset_neighbor_width_during_removal() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEW,
                Panel::Properties,
                DockTarget::Split {
                    group: 5,
                    edge: Edge::Right,
                },
            )
            .unwrap();
        layout
            .move_panel(
                VIEW,
                Panel::Adjustments,
                DockTarget::Split {
                    group: 5,
                    edge: Edge::Left,
                },
            )
            .unwrap();
        let group = layout.panel_group(Panel::Properties).unwrap();
        layout.set_column_collapsed(group, true, VIEW).unwrap();
        let before = layout.clone();
        let r = geometry(&layout);
        let original = r.groups.iter().find(|g| g.id == 5).unwrap().bounds.width;
        layout.set_panel_visible(Panel::Adjustments, false).unwrap();
        layout.reclaim_removed_columns(&before, &r);
        layout.validate().unwrap();
        let r = geometry(&layout);
        assert!(
            (r.groups.iter().find(|g| g.id == 5).unwrap().bounds.width - original).abs() < 0.01
        );
        assert_eq!(r.collapsed[0].bounds.width, TILE_SIZE);
    }

    #[test]
    fn collapse_is_undoable_without_changing_document_or_tab_state() {
        let mut state = crate::WorkspaceState::default();
        state.layout.select_tab(8, Panel::Properties).unwrap();
        let before = state.clone();
        let mut history = crate::workspace::WorkspaceHistory::default();
        history.begin(&state);
        state.layout.set_column_collapsed(8, true, VIEW).unwrap();
        history.finish(&state);
        let collapsed = state.clone();
        history.undo(&mut state);
        assert_eq!(state, before);
        history.redo(&mut state);
        assert_eq!(state, collapsed);
        let col = &geometry(&state.layout).collapsed[0];
        assert_eq!(col.groups[0].active, Panel::Properties);
        state.layout.select_tab(8, Panel::Adjustments).unwrap();
        assert_eq!(
            geometry(&state.layout).collapsed[0].groups[0].active,
            Panel::Adjustments
        );
        state
            .layout
            .reset_docking(crate::Platform::Generic)
            .unwrap();
        assert!(state.layout.collapsed.is_empty());
        state.validate().unwrap();
    }

    #[test]
    fn strip_controls_stay_bounded_and_overflow_is_not_a_drop_target() {
        let mut layout = DockLayout::default();
        layout.set_column_collapsed(8, true, VIEW).unwrap();
        for height in [10., 30., 80., 120., 400.] {
            let bounds = Bounds {
                x: 5.,
                y: 12.,
                width: TILE_SIZE,
                height,
            };
            let c = resolve_column(layout.node(8).unwrap(), bounds);
            for b in [c.expand, c.grip, c.content, c.empty] {
                assert!(b.x >= bounds.x && b.x + b.width <= bounds.x + bounds.width);
                assert!(b.y >= bounds.y && b.y + b.height <= bounds.y + bounds.height);
            }
            assert!(c.expand.y + c.expand.height <= c.grip.y);
            for icon in c.groups.iter().flat_map(|g| &g.icons) {
                let point = [icon.bounds.x + 18., icon.bounds.y + 18.];
                if !c.content.contains(point[0], point[1]) {
                    assert!(c.drop_hint(point).is_none());
                }
            }
        }
    }

    #[test]
    fn two_collapsed_subcolumns_do_not_stretch_to_a_full_width_sibling() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEW,
                Panel::Properties,
                DockTarget::Split {
                    group: 5,
                    edge: Edge::Right,
                },
            )
            .unwrap();
        let right = layout.panel_group(Panel::Properties).unwrap();
        layout.add_panel_to_group(Panel::ToolSettings, 6).unwrap();
        let before = geometry(&layout);
        layout.set_column_collapsed(5, true, VIEW).unwrap();
        layout.set_column_collapsed(right, true, VIEW).unwrap();
        let r = geometry(&layout);
        assert_eq!(r.collapsed.len(), 2);
        assert!(r.collapsed.iter().all(|c| c.bounds.width == TILE_SIZE));
        assert!(
            r.collapsed[0]
                .bounds
                .intersection(r.collapsed[1].bounds)
                .is_none()
        );
        layout.validate().unwrap();
        layout.set_column_collapsed(right, false, VIEW).unwrap();
        layout.set_column_collapsed(5, false, VIEW).unwrap();
        let after = geometry(&layout);
        for g in &before.groups {
            let restored = after.groups.iter().find(|r| r.id == g.id).unwrap();
            assert!((restored.bounds.width - g.bounds.width).abs() < 0.01);
        }
    }
}
