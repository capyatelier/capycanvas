//! Content-sized toolbar stacks at the three anchors of a workspace edge.
//! These remain ordinary dock nodes: identity, detach, persistence and history
//! use the same path as full edge docks. Hosts only display published geometry.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeAlignment {
    Start,
    Center,
    End,
}
impl EdgeAlignment {
    fn index(self) -> usize {
        match self {
            Self::Start => 0,
            Self::Center => 1,
            Self::End => 2,
        }
    }
}

fn toolbar_nodes<'a>(node: &'a DockNode, out: &mut Vec<&'a DockNode>) {
    match node {
        DockNode::Tabs { .. } => out.push(node),
        DockNode::Split { first, second, .. } => {
            toolbar_nodes(first, out);
            toolbar_nodes(second, out);
        }
    }
}
fn ribbon_axis(edge: Edge) -> Axis {
    if edge.axis() == Axis::Horizontal {
        Axis::Vertical
    } else {
        Axis::Horizontal
    }
}
fn along(bounds: Bounds, axis: Axis) -> f32 {
    if axis == Axis::Horizontal {
        bounds.width
    } else {
        bounds.height
    }
}
fn slice(mut bounds: Bounds, axis: Axis, offset: f32, length: f32) -> Bounds {
    if axis == Axis::Horizontal {
        bounds.x += offset;
        bounds.width = length;
    } else {
        bounds.y += offset;
        bounds.height = length;
    }
    bounds
}

impl DockLayout {
    pub(crate) fn compact_band(&self, group: u32) -> Option<&DockBand> {
        self.bands
            .iter()
            .find(|b| b.alignment.is_some() && b.root.find(group).is_some())
    }
    pub(super) fn validate_compact_band(&self, band: &DockBand) -> Result<(), String> {
        fn valid(layout: &DockLayout, node: &DockNode, axis: Axis) -> bool {
            if layout.is_collapsed(node.id()) {
                return false;
            }
            match node {
                DockNode::Tabs { panels, .. } => {
                    panels.len() == 1 && panels[0].kind() == PanelKind::Tiles
                }
                DockNode::Split {
                    first,
                    second,
                    axis: split,
                    ..
                } => *split == axis && valid(layout, first, axis) && valid(layout, second, axis),
            }
        }
        if !valid(self, &band.root, ribbon_axis(band.edge))
            || self
                .bands
                .iter()
                .filter(|b| b.edge == band.edge && b.alignment == band.alignment)
                .count()
                != 1
        {
            return Err(
                "A compact edge region must contain one ordered stack of standalone toolbars"
                    .into(),
            );
        }
        Ok(())
    }
    pub(super) fn dock_compact_toolbar(
        &mut self,
        moving: DockNode,
        edge: Edge,
        alignment: EdgeAlignment,
    ) -> Result<(), String> {
        if let Some(index) = self
            .bands
            .iter()
            .position(|b| b.edge == edge && b.alignment == Some(alignment))
        {
            let id = self.allocate()?;
            let band = &mut self.bands[index];
            band.root = DockNode::Split {
                id,
                axis: ribbon_axis(edge),
                fraction: 0.5,
                first: Box::new(band.root.clone()),
                second: Box::new(moving),
            };
        } else {
            let id = self.allocate()?;
            self.bands.insert(
                0,
                DockBand {
                    id,
                    edge,
                    alignment: Some(alignment),
                    extent: TILE_SIZE + WORKSPACE_SPACING,
                    root: moving,
                },
            );
        }
        Ok(())
    }

    /// Resolve all three regions on an edge together. They share one strip,
    /// retain their natural lengths, and compress/wrap only when they collide.
    pub(super) fn resolve_compact_edge(
        &self,
        bands: &[DockBand],
        edge: Edge,
        remaining: &mut Bounds,
        result: &mut ResolvedLayout,
    ) {
        let axis = ribbon_axis(edge);
        let length = along(*remaining, axis);
        if length <= 0. {
            return;
        }
        let mut runs: Vec<_> = bands
            .iter()
            .filter(|b| b.edge == edge && b.alignment.is_some())
            .map(|b| {
                let mut nodes = Vec::new();
                toolbar_nodes(&b.root, &mut nodes);
                let lengths: Vec<_> = nodes
                    .iter()
                    .map(|n| {
                        let DockNode::Tabs { active, .. } = n else {
                            unreachable!()
                        };
                        let config = self.panel(*active).unwrap();
                        toolbar_span(
                            config.tiles(),
                            config.tile_style.size()[usize::from(axis == Axis::Vertical)],
                            axis,
                        ) + 20.
                    })
                    .collect();
                (b.alignment.unwrap(), nodes, lengths)
            })
            .collect();
        runs.sort_by_key(|r| r.0.index());
        let gap = WORKSPACE_SPACING.min(length / (runs.len() as f32 * 2.).max(1.));
        let total: f32 = runs
            .iter()
            .map(|(_, nodes, lengths)| {
                lengths.iter().sum::<f32>() + gap * nodes.len().saturating_sub(1) as f32
            })
            .sum();
        let available = (length - gap * runs.len().saturating_sub(1) as f32).max(0.);
        let scale = (available / total.max(1.)).min(1.);
        let mut sizes = [0.; 3];
        for (alignment, nodes, lengths) in &mut runs {
            let natural = lengths.iter().sum::<f32>();
            let size = (natural + gap * nodes.len().saturating_sub(1) as f32) * scale;
            let inner_gap = gap.min(size / (nodes.len() as f32 * 2.).max(1.));
            let factor =
                (size - inner_gap * nodes.len().saturating_sub(1) as f32).max(0.) / natural.max(1.);
            for l in lengths {
                *l *= factor;
            }
            sizes[alignment.index()] = size;
        }
        let start_end = sizes[0] + if sizes[0] > 0. { gap } else { 0. };
        let end_start = length - sizes[2] - if sizes[2] > 0. { gap } else { 0. };
        let offsets = [
            0.,
            ((length - sizes[1]) * 0.5)
                .max(start_end)
                .min((end_start - sizes[1]).max(start_end)),
            length - sizes[2],
        ];
        let cross_size = |node: &DockNode, length: f32| {
            let DockNode::Tabs { active, .. } = node else {
                unreachable!()
            };
            let config = self.panel(*active).unwrap();
            toolbar_cross_extent(length, config.tiles(), config.tile_style, axis)
        };
        let cross = runs
            .iter()
            .flat_map(|(_, nodes, lengths)| nodes.iter().zip(lengths))
            .map(|(n, l)| cross_size(n, *l))
            .fold(TILE_SIZE, f32::max);
        let capacity = if axis == Axis::Horizontal {
            remaining.height
        } else {
            remaining.width
        };
        let extent = (cross + WORKSPACE_SPACING).min(capacity.max(0.));
        let mut strip = remaining.strip(edge, extent);
        strip.strip(
            match edge {
                Edge::Top => Edge::Bottom,
                Edge::Bottom => Edge::Top,
                Edge::Left => Edge::Right,
                Edge::Right => Edge::Left,
            },
            WORKSPACE_SPACING.min(extent),
        );
        if strip.width <= 0. || strip.height <= 0. {
            return;
        }
        if !result.reveal_edges.contains(&edge) {
            result.reveal_edges.push(edge);
        }
        for (alignment, nodes, lengths) in runs {
            let size = sizes[alignment.index()];
            let gap = gap.min(size / (nodes.len() as f32 * 2.).max(1.));
            let mut offset = offsets[alignment.index()];
            for (node, length) in nodes.into_iter().zip(lengths) {
                let mut b = slice(strip, axis, offset, length);
                let cross = cross_size(node, length);
                if axis == Axis::Vertical {
                    let width = b.width.min(cross);
                    if edge == Edge::Right {
                        b.x += b.width - width;
                    }
                    b.width = width;
                } else {
                    let height = b.height.min(cross);
                    if edge == Edge::Bottom {
                        b.y += b.height - height;
                    }
                    b.height = height;
                }
                resolve_node(node, b, axis, self, result, false);
                offset += length + gap;
            }
        }
    }

    /// Match the near-edge target to the visible toolbar's leading edge. Its
    /// grip can be well inside the window when a medium/large preview touches
    /// the edge. Without a live drag, use the ordinary pointer hit area.
    pub fn compact_edge_drop_hint(
        &self,
        resolved: &ResolvedLayout,
        item: DockItem,
        point: [f32; 2],
        preview: Option<Bounds>,
    ) -> Option<DropHint> {
        let toolbar = match item {
            DockItem::Panel { panel } => panel.kind() == PanelKind::Tiles,
            DockItem::Group { group } => self
                .group_panels(group)
                .is_ok_and(|p| p.len() == 1 && p[0].kind() == PanelKind::Tiles),
            _ => false,
        };
        if !toolbar {
            return None;
        }
        // Existing tab/column insertion surfaces keep their native hit areas.
        // Compact anchors occupy the free edge, not the controls of a sidebar.
        if resolved.collapsed.iter().any(|c| {
            Bounds {
                y: c.bounds.y - WORKSPACE_SPACING,
                height: c.bounds.height + WORKSPACE_SPACING * 2.,
                ..c.bounds
            }
            .contains(point[0], point[1])
        }) || resolved.groups.iter().any(|g| {
            g.tabs_visible
                && Bounds {
                    height: TAB_BAR_HEIGHT,
                    ..g.bounds
                }
                .contains(point[0], point[1])
        }) {
            return None;
        }
        // Existing region ends stay stack targets, including in the small gap
        // outside the root. Each toolbar remains independently draggable.
        for group in resolved.groups.iter().rev() {
            let Some(band) = self.compact_band(group.id) else {
                continue;
            };
            let axis = ribbon_axis(band.edge);
            for (before, edge) in [
                (
                    true,
                    if axis == Axis::Horizontal {
                        Edge::Left
                    } else {
                        Edge::Top
                    },
                ),
                (
                    false,
                    if axis == Axis::Horizontal {
                        Edge::Right
                    } else {
                        Edge::Bottom
                    },
                ),
            ] {
                let b = group.bounds;
                let position = if before { 0. } else { along(b, axis) };
                let hit = slice(b, axis, position - 12., 24.);
                if hit.contains(point[0], point[1]) {
                    return Some(DropHint {
                        target: DockTarget::Split {
                            group: group.id,
                            edge,
                        },
                        bounds: slice(b, axis, position - 1.5, 3.),
                    });
                }
            }
        }
        let top = self.header_presentation.height.max(crate::HEADER_HEIGHT);
        let bounds = Bounds {
            x: 0.,
            y: top,
            width: resolved.viewport[0],
            height: (self.workspace_height(resolved.viewport[1]) - top).max(0.),
        };
        if point[0] < 0.
            || point[0] > bounds.width
            || point[1] < top
            || point[1] > resolved.viewport[1]
        {
            return None;
        }
        // Native footer/status insets must not create a dead strip between a
        // bottom target and the window edge. Keep title-bar targets separate.
        let point = [point[0], point[1].min(bounds.y + bounds.height)];
        let distance_to = |edge, b: Bounds| match edge {
            Edge::Left => b.x - bounds.x,
            Edge::Right => bounds.x + bounds.width - b.x - b.width,
            Edge::Top => b.y - bounds.y,
            Edge::Bottom => bounds.y + bounds.height - b.y - b.height,
        };
        let contact = Bounds {
            x: point[0],
            y: point[1],
            ..Bounds::default()
        };
        let (pointer_distance, pointer_edge) = nearest_edge(bounds, point[0], point[1]);
        let edge = preview
            .filter(|_| pointer_distance > WORKSPACE_SPACING * 4.)
            .map_or_else(
                || pointer_edge,
                |b| {
                    [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]
                        .into_iter()
                        .min_by(|a, c| {
                            distance_to(*a, b)
                                .abs()
                                .total_cmp(&distance_to(*c, b).abs())
                                .then_with(|| {
                                    distance_to(*a, contact).total_cmp(&distance_to(*c, contact))
                                })
                        })
                        .unwrap()
                },
            );
        let distance = distance_to(edge, contact);
        let grab_inset = preview
            .map_or(0., |b| match edge {
                Edge::Left => point[0] - b.x,
                Edge::Right => b.x + b.width - point[0],
                Edge::Top => point[1] - b.y,
                Edge::Bottom => b.y + b.height - point[1],
            })
            .max(0.);
        let compact_reach = (grab_inset + WORKSPACE_SPACING).max(WORKSPACE_SPACING * 4.);
        if distance > compact_reach {
            // A compact bar's body must not mask the broader full-edge target.
            // End stacking was already considered above.
            return (distance <= compact_reach + PANEL_SNAP_DISTANCE
                && (preview.is_some()
                    || self
                        .bands
                        .iter()
                        .any(|b| b.edge == edge && b.alignment.is_some())))
            .then(|| DropHint {
                target: DockTarget::Edge { edge, outer: true },
                bounds: edge_line(bounds, edge),
            });
        }
        let axis = ribbon_axis(edge);
        let length = along(bounds, axis);
        let coordinate = if axis == Axis::Horizontal {
            point[0] - bounds.x
        } else {
            point[1] - bounds.y
        };
        let reach = 48_f32.min(length / 6.);
        let (alignment, offset) = if coordinate <= reach * 2. {
            (EdgeAlignment::Start, 0.)
        } else if coordinate >= length - reach * 2. {
            (EdgeAlignment::End, length - reach * 2.)
        } else if (coordinate - length / 2.).abs() <= reach {
            (EdgeAlignment::Center, length / 2. - reach)
        } else {
            return None;
        };
        let mut hint = slice(bounds, axis, offset, reach * 2.);
        hint = hint.strip(edge, WORKSPACE_SPACING * 4.);
        Some(DropHint {
            target: DockTarget::CompactEdge { edge, alignment },
            bounds: hint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VIEWPORT: [f32; 2] = [1200., 900.];
    fn layout() -> DockLayout {
        let mut layout = DockLayout::default();
        layout.bands.clear();
        layout.floating.clear();
        layout
    }
    fn add(layout: &mut DockLayout, count: usize, edge: Edge, alignment: EdgeAlignment) -> Panel {
        let panel = layout
            .add_toolbar(
                None,
                &format!("Test {}", layout.panels.len()),
                &vec![ToolbarControl::Color; count],
            )
            .unwrap();
        layout
            .move_panel(VIEWPORT, panel, DockTarget::CompactEdge { edge, alignment })
            .unwrap();
        panel
    }
    fn resolved(layout: &DockLayout, viewport: [f32; 2]) -> ResolvedLayout {
        layout.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        )
    }
    #[test]
    fn compact_regions_align_share_space_and_remain_distinct_toolbars() {
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let mut l = layout();
            let a = add(&mut l, 2, edge, EdgeAlignment::Center);
            let b = add(&mut l, 3, edge, EdgeAlignment::Center);
            let r = resolved(&l, VIEWPORT);
            let a = r.groups.iter().find(|g| g.active == a).unwrap();
            let b = r.groups.iter().find(|g| g.active == b).unwrap();
            for group in [a, b] {
                assert!(l.column_for_group(group.id).is_none());
                assert!(l.collapsible_column_for_group(group.id).is_none());
            }
            let before = l.clone();
            assert!(
                l.set_column_collapsed(l.bands[0].root.id(), true, VIEWPORT)
                    .is_err()
            );
            assert_eq!(l, before, "compact stacks do not become collapsed columns");
            let axis = ribbon_axis(edge);
            assert_eq!(along(a.bounds, axis), 96.);
            assert_eq!(along(b.bounds, axis), 134.);
            let (start, end, center) = if axis == Axis::Vertical {
                (
                    a.bounds.y,
                    b.bounds.y + b.bounds.height,
                    (crate::HEADER_HEIGHT + VIEWPORT[1] - WORKSPACE_SPACING) * 0.5,
                )
            } else {
                (a.bounds.x, b.bounds.x + b.bounds.width, VIEWPORT[0] * 0.5)
            };
            assert!(
                (start + end - center * 2.).abs() < 0.01,
                "{edge:?}: {a:?} {b:?}"
            );
            assert!((end - start - 96. - 134. - WORKSPACE_SPACING).abs() < 0.01);
            let work = r.work_area;
            add(&mut l, 1, edge, EdgeAlignment::Start);
            add(&mut l, 1, edge, EdgeAlignment::End);
            let r = resolved(&l, VIEWPORT);
            assert_eq!(r.work_area, work, "three anchors share one strip");
            for (i, g) in r.groups.iter().enumerate() {
                for other in &r.groups[i + 1..] {
                    assert!(g.bounds.intersection(other.bounds).is_none());
                }
            }
            l.validate().unwrap();
            assert_eq!(
                l,
                serde_json::from_str::<DockLayout>(&serde_json::to_string(&l).unwrap()).unwrap()
            );
        }
    }
    #[test]
    fn compact_stacks_reorder_at_ends_and_can_return_to_full_edge_or_float() {
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let mut l = layout();
            let a = add(&mut l, 2, edge, EdgeAlignment::Center);
            let b = add(&mut l, 2, edge, EdgeAlignment::End);
            let group = l.panel_group(a).unwrap();
            let before_edge = if ribbon_axis(edge) == Axis::Vertical {
                Edge::Top
            } else {
                Edge::Left
            };
            l.move_panel(
                VIEWPORT,
                b,
                DockTarget::Split {
                    group,
                    edge: before_edge,
                },
            )
            .unwrap();
            let r = resolved(&l, VIEWPORT);
            assert_eq!(r.groups[0].active, b);
            assert_eq!(r.groups[1].active, a);
            assert_eq!(l.bands.len(), 1);
            let unchanged = l.clone();
            assert!(
                l.move_panel(
                    VIEWPORT,
                    Panel::Layers,
                    DockTarget::CompactEdge {
                        edge,
                        alignment: EdgeAlignment::Start
                    }
                )
                .is_err()
            );
            assert!(
                l.move_panel(VIEWPORT, b, DockTarget::Tab { group, index: None })
                    .is_err()
            );
            assert_eq!(l, unchanged);
            l.move_panel(VIEWPORT, b, DockTarget::Edge { edge, outer: true })
                .unwrap();
            assert!(l.compact_band(l.panel_group(b).unwrap()).is_none());
            l.move_panel(
                VIEWPORT,
                a,
                DockTarget::Float {
                    position: [400., 300.],
                },
            )
            .unwrap();
            assert!(
                resolved(&l, VIEWPORT)
                    .groups
                    .iter()
                    .any(|g| g.active == a && g.floating)
            );
            l.validate().unwrap();
        }
    }
    #[test]
    fn compact_targets_follow_the_visible_preview_in_every_orientation() {
        let mut l = layout();
        let source = add(&mut l, 2, Edge::Left, EdgeAlignment::Center);
        let r = resolved(&l, VIEWPORT);
        let item = DockItem::Panel { panel: source };
        for (edge, point, preview) in [
            (
                Edge::Right,
                [1146., 450.],
                Bounds {
                    x: 1092.,
                    y: 180.,
                    width: 108.,
                    height: 280.,
                },
            ),
            (
                Edge::Left,
                [54., 450.],
                Bounds {
                    x: 0.,
                    y: 180.,
                    width: 108.,
                    height: 280.,
                },
            ),
            (
                Edge::Top,
                [600., 318.],
                Bounds {
                    x: 546.,
                    y: 48.,
                    width: 108.,
                    height: 280.,
                },
            ),
            (
                Edge::Bottom,
                [600., 890.],
                Bounds {
                    x: 546.,
                    y: 620.,
                    width: 108.,
                    height: 280.,
                },
            ),
        ] {
            assert_eq!(
                l.compact_edge_drop_hint(&r, item, point, Some(preview))
                    .unwrap()
                    .target,
                DockTarget::CompactEdge {
                    edge,
                    alignment: EdgeAlignment::Center
                }
            );
            let mut farther = preview;
            let mut p = point;
            match edge {
                Edge::Left => {
                    farther.x += 32.;
                    p[0] += 32.;
                }
                Edge::Right => {
                    farther.x -= 32.;
                    p[0] -= 32.;
                }
                Edge::Top => {
                    farther.y += 32.;
                    p[1] += 32.;
                }
                Edge::Bottom => {
                    farther.y -= 32.;
                    p[1] -= 32.;
                }
            }
            assert!(
                matches!(l.compact_edge_drop_hint(&r, item, p, Some(farther)).unwrap().target,
                DockTarget::Edge { edge: actual, .. } if actual == edge)
            );
        }
    }

    #[test]
    fn top_corner_uses_the_visible_edge_even_with_a_bottom_grip() {
        let mut l = layout();
        let source = add(&mut l, 2, Edge::Left, EdgeAlignment::Center);
        let r = resolved(&l, VIEWPORT);
        let item = DockItem::Panel { panel: source };
        assert_eq!(
            l.compact_edge_drop_hint(
                &r,
                item,
                [80., 318.],
                Some(Bounds {
                    x: 26.,
                    y: 48.,
                    width: 108.,
                    height: 280.,
                })
            )
            .unwrap()
            .target,
            DockTarget::CompactEdge {
                edge: Edge::Top,
                alignment: EdgeAlignment::Start
            }
        );
        assert_eq!(
            l.compact_edge_drop_hint(
                &r,
                item,
                [1091., 128.],
                Some(Bounds {
                    x: 982.,
                    y: -302.,
                    width: 218.,
                    height: 440.,
                })
            )
            .unwrap()
            .target,
            DockTarget::CompactEdge {
                edge: Edge::Right,
                alignment: EdgeAlignment::Start
            }
        );
    }
    #[test]
    fn compact_targets_are_nearer_and_shorter_than_full_edge_targets() {
        let mut l = layout();
        let source = add(&mut l, 2, Edge::Left, EdgeAlignment::Center);
        let r = resolved(&l, VIEWPORT);
        let item = DockItem::Panel { panel: source };
        for (point, edge, alignment) in [
            ([1196., 450.], Edge::Right, EdgeAlignment::Center),
            ([1196., 64.], Edge::Right, EdgeAlignment::Start),
            ([1196., 885.], Edge::Right, EdgeAlignment::End),
            ([600., 50.], Edge::Top, EdgeAlignment::Center),
            ([600., 896.], Edge::Bottom, EdgeAlignment::Center),
        ] {
            let hint = l.compact_edge_drop_hint(&r, item, point, None).unwrap();
            assert_eq!(hint.target, DockTarget::CompactEdge { edge, alignment });
        }
        assert!(
            l.compact_edge_drop_hint(&r, item, [1170., 450.], None)
                .is_none()
        );
        assert!(matches!(
            r.drop_hint(1170., 450., &[], true).unwrap().target,
            DockTarget::Edge {
                edge: Edge::Right,
                ..
            }
        ));
        assert!(
            l.compact_edge_drop_hint(&r, item, [1196., 250.], None)
                .is_none()
        );
        assert!(
            l.compact_edge_drop_hint(
                &r,
                DockItem::Panel {
                    panel: Panel::Layers
                },
                [1196., 450.],
                None,
            )
            .is_none()
        );
        assert!(
            l.compact_edge_drop_hint(&r, item, [600., 10.], None)
                .is_none(),
            "native title bar is not a toolbar dock"
        );
    }
    #[test]
    fn compact_regions_compress_without_overlap_at_small_viewports_and_all_styles() {
        for style in [
            TileStyle::Small,
            TileStyle::Medium,
            TileStyle::Large,
            TileStyle::MediumLabeled,
            TileStyle::Labeled,
        ] {
            for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                let mut l = layout();
                for alignment in [
                    EdgeAlignment::Start,
                    EdgeAlignment::Center,
                    EdgeAlignment::End,
                ] {
                    let p = add(&mut l, 3, edge, alignment);
                    l.set_tile_style(p, style, VIEWPORT).unwrap();
                    let before = l.clone();
                    l.double_click_panel_handle(l.panel_group(p).unwrap(), VIEWPORT)
                        .unwrap();
                    assert_eq!(l, before, "compact handles already fit their content");
                }
                for viewport in [[640., 480.], [900., 640.], VIEWPORT] {
                    let r = resolved(&l, viewport);
                    for (i, g) in r.groups.iter().enumerate() {
                        let b = g.bounds;
                        assert!(
                            b.x >= 0.
                                && b.y >= 0.
                                && b.x + b.width <= viewport[0] + 0.01
                                && b.y + b.height <= viewport[1] + 0.01,
                            "{style:?} {edge:?} {viewport:?}: {b:?}"
                        );
                        for o in &r.groups[i + 1..] {
                            assert!(
                                b.intersection(o.bounds).is_none(),
                                "{style:?} {edge:?}: {b:?} {:?}",
                                o.bounds
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn existing_compact_bar_does_not_mask_full_edge_docking() {
        let mut l = layout();
        add(&mut l, 4, Edge::Right, EdgeAlignment::Center);
        let source = add(&mut l, 2, Edge::Left, EdgeAlignment::Center);
        let r = resolved(&l, VIEWPORT);
        let hint = l
            .compact_edge_drop_hint(
                &r,
                DockItem::Panel { panel: source },
                [VIEWPORT[0] - 28., 470.],
                None,
            )
            .unwrap();
        assert!(matches!(
            hint.target,
            DockTarget::Edge {
                edge: Edge::Right,
                ..
            }
        ));
        l.move_panel(VIEWPORT, source, hint.target).unwrap();
        assert!(l.compact_band(l.panel_group(source).unwrap()).is_none());
    }
    #[test]
    fn compact_targets_cover_practical_edge_approaches_and_native_footer() {
        let mut l = layout();
        let source = add(&mut l, 2, Edge::Left, EdgeAlignment::Center);
        l.bottom_inset = 36.;
        let r = resolved(&l, VIEWPORT);
        let item = DockItem::Panel { panel: source };
        for (point, edge, alignment) in [
            ([1180., 450.], Edge::Right, EdgeAlignment::Center),
            ([1180., 88.], Edge::Right, EdgeAlignment::Start),
            ([1198., 828.], Edge::Right, EdgeAlignment::End),
            ([600., 898.], Edge::Bottom, EdgeAlignment::Center),
        ] {
            let hint = l.compact_edge_drop_hint(&r, item, point, None).unwrap();
            assert_eq!(hint.target, DockTarget::CompactEdge { edge, alignment });
        }
    }
}
