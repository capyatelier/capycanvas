//! Collapsed column stacks. Opening a member reuses its ordinary dock tree;
//! stack preferences never introduce a second panel layout or resize model.
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "StackWire")]
pub struct ColumnStack {
    pub column: u32,
    pub members: Vec<u32>,
    pub drawers: bool,
    pub auto_hide: bool,
    #[serde(skip)]
    pub open_column: Option<u32>,
}

// Read the old per-column preferences once. Obsolete width/height overrides
// are ignored: expanded members use the width and splits of their dock tree.
#[derive(Deserialize)]
struct StackWire {
    column: u32,
    #[serde(default)]
    members: Option<Vec<u32>>,
    #[serde(default)]
    drawers: Option<bool>,
    #[serde(default)]
    auto_hide: bool,
    #[serde(default)]
    mode: Option<String>,
}
impl From<StackWire> for ColumnStack {
    fn from(w: StackWire) -> Self {
        Self {
            column: w.column,
            members: w.members.unwrap_or_else(|| vec![w.column]),
            drawers: w.drawers.unwrap_or_else(|| w.mode.as_deref().is_some_and(|m| m != "group_panel")),
            auto_hide: w.auto_hide,
            open_column: None,
        }
    }
}
impl ColumnStack {
    pub fn single(column: u32) -> Self {
        Self {
            column,
            members: vec![column],
            drawers: false,
            auto_hide: false,
            open_column: None,
        }
    }
}

/// Only the placement/attachment of an open member. Its content and dividers
/// are ordinary entries in ResolvedLayout, rendered by the normal dock views.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenColumn {
    pub column: u32,
    pub bounds: Bounds,
    pub direction: Edge,
    pub connections: Vec<(Panel, crate::DrawerConnection)>,
}

impl ResolvedLayout {
    /// Empty space and footer grips append a new member; the gap between
    /// members inserts there. The last tile's boundary appends a group within
    /// its member before the empty-space/handle target can claim the contact.
    pub(crate) fn stack_item_drop_hint(&self, point: [f32; 2]) -> Option<DropHint> {
        if self
            .groups
            .iter()
            .any(|g| g.bounds.contains(point[0], point[1]))
        {
            return None;
        }
        for member in &self.collapsed {
            if let Some(hint) = member.append_group_drop_hint(point) {
                return Some(hint);
            }
            if member.empty.contains(point[0], point[1]) || member.grip.contains(point[0], point[1])
            {
                return Some(DropHint {
                    target: DockTarget::StackColumn {
                        column: member.id,
                        before: false,
                    },
                    bounds: edge_line(member.bounds, Edge::Bottom),
                });
            }
        }
        for pair in self.collapsed.windows(2) {
            let (above, below) = (&pair[0], &pair[1]);
            let gap = Bounds {
                x: above.bounds.x,
                y: above.bounds.y + above.bounds.height,
                width: above.bounds.width,
                height: below.bounds.y - above.bounds.y - above.bounds.height,
            };
            if above.stack == below.stack && gap.contains(point[0], point[1]) {
                let height = gap.height.min(3.);
                return Some(DropHint {
                    target: DockTarget::StackColumn {
                        column: above.id,
                        before: false,
                    },
                    bounds: Bounds {
                        y: gap.y + (gap.height - height) * 0.5,
                        height,
                        ..gap
                    },
                });
            }
        }
        None
    }

    pub fn open_column_at_divider(&self, id: u32) -> Option<u32> {
        let d = self
            .dividers
            .iter()
            .find(|d| d.id == id && d.axis == Axis::Horizontal)?;
        self.collapsed.iter().find_map(|c| {
            let open = c.open.as_ref()?;
            let gap = match open.direction {
                Edge::Right => d.bounds.x - open.bounds.x - open.bounds.width,
                Edge::Left => open.bounds.x - d.bounds.x - d.bounds.width,
                _ => return None,
            };
            ((gap - WORKSPACE_SPACING).abs() < 0.5
                && open.bounds.y < d.bounds.y + d.bounds.height
                && d.bounds.y < open.bounds.y + open.bounds.height)
                .then_some(open.column)
        })
    }
}

impl DockLayout {
    pub(crate) fn column_contains(&self, column: u32, node: u32) -> bool {
        self.node(column).is_some_and(|n| n.find(node).is_some())
    }
    pub(super) fn stack_column(
        &mut self,
        viewport: [f32; 2],
        source: u32,
        target: u32,
        before: bool,
    ) -> Result<(), String> {
        if source == target {
            return Ok(());
        }
        if !self.is_collapsed(source) || !self.is_collapsed(target) {
            return Err("Only collapsed columns can be stacked".into());
        }
        if !self.column_stack(source).members.contains(&source)
            || !self.column_stack(target).members.contains(&target)
        {
            return Err("Move individual stack members".into());
        }
        let moving = self.node(source).ok_or("Unknown source column")?.clone();
        if moving.find(target).is_some() {
            return Err("A column cannot contain itself".into());
        }
        let panels: Vec<_> = self
            .panels
            .iter()
            .filter_map(|p| moving.group_for(p.id).map(|_| p.id))
            .collect();
        let collapsed = self.collapsed.iter().find(|c| c.root == source).unwrap().clone();
        let geometry = self.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        let mut next = self.clone();
        next.detach(&panels);
        next.reclaim_removed_columns(self, &geometry);
        next.collapsed.push(collapsed);
        next.insert_stack_member(moving, target, before)?;
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Insert an already detached, collapsed tree using the destination's
    /// preferences. Callers publish the completed move as one transaction.
    pub(super) fn insert_stack_member(
        &mut self,
        moving: DockNode,
        target: u32,
        before: bool,
    ) -> Result<(), String> {
        let source = moving.id();
        let mut stack = self.column_stack(target);
        let old_root = stack.column;
        let index = stack
            .members
            .iter()
            .position(|m| *m == target)
            .ok_or("Unknown target member")?
            + usize::from(!before);
        stack.members.insert(index, source);
        let mut members = Vec::new();
        for member in &stack.members {
            members.push(if *member == source {
                moving.clone()
            } else {
                self.node(*member).ok_or("Unknown stack member")?.clone()
            });
        }
        let mut tree = members.remove(0);
        for member in members {
            tree = DockNode::Split {
                id: self.allocate()?,
                axis: Axis::Vertical,
                fraction: 0.5,
                first: Box::new(tree),
                second: Box::new(member),
            };
        }
        let root = tree.id();
        *self.node_mut(old_root).ok_or("Unknown target stack")? = tree;
        if !stack.members.contains(&old_root) {
            self.collapsed.retain(|c| c.root != old_root);
        }
        let width = stack
            .members
            .iter()
            .map(|m| self.expanded_column_width(*m))
            .fold(128., f32::max);
        self.collapsed.push(CollapsedColumn {
            root,
            expanded_width: width,
        });
        self.column_stacks
            .retain(|s| s.column != old_root && !stack.members.contains(&s.column));
        stack.column = root;
        self.column_stacks.push(stack);
        Ok(())
    }

    pub fn column_stack(&self, column: u32) -> ColumnStack {
        self.column_stacks
            .iter()
            .find(|s| s.column == column || s.members.contains(&column))
            .cloned()
            .unwrap_or_else(|| ColumnStack::single(column))
    }
    pub(crate) fn column_stack_mut(&mut self, column: u32) -> &mut ColumnStack {
        let index = self
            .column_stacks
            .iter()
            .position(|s| s.column == column || s.members.contains(&column))
            .unwrap_or_else(|| {
                self.column_stacks.push(ColumnStack::single(column));
                self.column_stacks.len() - 1
            });
        &mut self.column_stacks[index]
    }
    pub(crate) fn column_roots(&self) -> Vec<u32> {
        let mut roots: Vec<_> = self
            .panels
            .iter()
            .filter_map(|p| self.panel_group(p.id))
            .filter_map(|g| self.collapsible_column_for_group(g))
            .collect();
        roots.extend(
            self.collapsed
                .iter()
                .map(|c| self.column_stack(c.root).column),
        );
        roots.sort_unstable();
        roots.dedup();
        roots
    }
    pub(crate) fn open_stack_column(&self, column: u32) -> Option<u32> {
        let s = self.column_stacks.iter().find(|s| s.column == column)?;
        let member = s.open_column?;
        (!s.drawers
            && self.is_collapsed(column)
            && s.members.contains(&member)
            && self.node(column)?.find(member).is_some())
        .then_some(member)
    }
    pub(crate) fn expanded_column_width(&self, column: u32) -> f32 {
        self.collapsed
            .iter()
            .find(|c| c.root == column)
            .map_or(240., |c| c.expanded_width)
    }
    pub(super) fn projected_column_bands(&self, base: &ResolvedLayout) -> Vec<DockBand> {
        fn width(node: &mut DockNode, layout: &DockLayout, base: &ResolvedLayout) -> f32 {
            if layout.is_collapsed(node.id()) {
                return TILE_SIZE
                    + layout.open_stack_column(node.id()).map_or(0., |c| {
                        layout.expanded_column_width(c) + WORKSPACE_SPACING * 2.
                    });
            }
            match node {
                DockNode::Tabs { id, .. } => base
                    .groups
                    .iter()
                    .find(|g| g.id == *id)
                    .map_or(TILE_SIZE, |g| g.bounds.width),
                DockNode::Split {
                    axis,
                    fraction,
                    first,
                    second,
                    ..
                } => {
                    let a = width(first, layout, base);
                    let b = width(second, layout, base);
                    if *axis == Axis::Horizontal {
                        *fraction = a / (a + b).max(1.);
                        a + b + WORKSPACE_SPACING
                    } else {
                        a.max(b)
                    }
                }
            }
        }
        let mut bands = self.bands.clone();
        for band in &mut bands {
            if self.column_stacks.iter().any(|s| {
                self.open_stack_column(s.column).is_some() && band.root.find(s.column).is_some()
            }) {
                band.extent = width(&mut band.root, self, base) + WORKSPACE_SPACING;
            }
        }
        bands
    }

    pub(super) fn resolve_stack(
        &self,
        node: &DockNode,
        bounds: Bounds,
        result: &mut ResolvedLayout,
        opened: bool,
    ) {
        let stack = self.column_stack(node.id());
        let open = opened.then(|| self.open_stack_column(node.id())).flatten();
        let direction = self
            .bands
            .iter()
            .find(|b| b.root.find(node.id()).is_some())
            .map_or(Edge::Right, |b| {
                if b.edge == Edge::Right {
                    Edge::Left
                } else {
                    Edge::Right
                }
            });
        let strip = Bounds {
            x: if direction == Edge::Left {
                bounds.x + bounds.width - TILE_SIZE.min(bounds.width)
            } else {
                bounds.x
            },
            width: TILE_SIZE.min(bounds.width),
            ..bounds
        };
        let members: Vec<_> = stack
            .members
            .iter()
            .filter_map(|id| node.find(*id))
            .collect();
        let mut y = strip.y;
        let natural: Vec<_> = members
            .iter()
            .map(|n| {
                let c = columns::resolve_column(n, strip);
                c.groups.last().map_or(PANEL_GRIP_HEIGHT, |g| {
                    g.bounds.y + g.bounds.height - strip.y
                }) + WORKSPACE_SPACING
                    + PANEL_GRIP_HEIGHT
            })
            .collect();
        let total: f32 = natural.iter().sum();
        let gaps = members.len().saturating_sub(1) as f32;
        let gap = WORKSPACE_SPACING.min(strip.height / gaps.max(1.));
        let available = (strip.height - gap * gaps).max(0.);
        for (index, member) in members.iter().enumerate() {
            let last = index + 1 == members.len();
            let h = if last {
                strip.y + strip.height - y
            } else {
                natural[index] * (available / total.max(1.)).min(1.)
            };
            let mut c = columns::resolve_column(
                member,
                Bounds {
                    y,
                    height: h.max(0.),
                    ..strip
                },
            );
            c.stack = stack.column;
            c.scroll(
                self.column_scroll
                    .iter()
                    .find(|(id, _)| *id == member.id())
                    .map_or(0., |(_, v)| *v),
            );
            if open == Some(member.id()) {
                let body = Bounds {
                    x: if direction == Edge::Right {
                        strip.x + strip.width + WORKSPACE_SPACING
                    } else {
                        bounds.x + WORKSPACE_SPACING
                    },
                    y: 0.,
                    width: (bounds.width - strip.width - WORKSPACE_SPACING * 2.).max(0.),
                    height: result.viewport[1],
                };
                let mut expanded = self.clone();
                expanded.collapsed.retain(|c| c.root != member.id());
                resolve_node(member, body, Axis::Vertical, &expanded, result, false);
                let connections = c
                    .groups
                    .iter()
                    .filter_map(|g| {
                        let icon = g.icons.iter().find(|i| i.panel == g.active)?;
                        let anchor = icon.bounds.intersection(c.content)?;
                        crate::DrawerPlacement {
                            bounds: body,
                            anchor,
                            direction,
                            columns: Vec::new(),
                        }
                        .connection()
                        .map(|connection| (g.active, connection))
                    })
                    .collect();
                c.open = Some(OpenColumn {
                    column: member.id(),
                    bounds: body,
                    direction,
                    connections,
                });
            }
            result.collapsed.push(c);
            y += h + gap;
        }
    }
}
