//! Per-column presentation preferences and a full-height projection of one tab
//! group. The dock tree stays intact; opening only changes resolved allocations.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnMode {
    #[default]
    Drawers,
    GroupPanel,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ColumnSettings {
    pub column: u32,
    #[serde(default)]
    pub mode: ColumnMode,
    #[serde(default)]
    pub auto_hide: bool,
    #[serde(default)]
    pub width: Option<f32>,
    #[serde(default)]
    pub heights: Vec<ColumnPanelHeight>,
    /// Hosts opt into the presentation by opening it. Saved workspaces and
    /// hosts without this renderer still retain the portable preferences.
    #[serde(skip)]
    pub open_group: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnPanelHeight {
    pub group: u32,
    pub panel: Panel,
    pub weight: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnGroupPanel {
    pub group: u32,
    pub bounds: Bounds,
    pub panels: Vec<ColumnIcon>,
    pub dividers: Vec<Bounds>,
    pub resize: Bounds,
    pub direction: Edge,
}

impl DockLayout {
    pub fn column_settings(&self, column: u32) -> ColumnSettings {
        self.column_settings
            .iter()
            .find(|s| s.column == column)
            .cloned()
            .unwrap_or(ColumnSettings {
                column,
                ..Default::default()
            })
    }
    pub(crate) fn column_settings_mut(&mut self, column: u32) -> &mut ColumnSettings {
        let index = self
            .column_settings
            .iter()
            .position(|s| s.column == column)
            .unwrap_or_else(|| {
                self.column_settings.push(ColumnSettings {
                    column,
                    ..Default::default()
                });
                self.column_settings.len() - 1
            });
        &mut self.column_settings[index]
    }
    pub(crate) fn column_roots(&self) -> Vec<u32> {
        let mut roots: Vec<_> = self
            .panels
            .iter()
            .filter_map(|p| self.panel_group(p.id))
            .filter_map(|g| self.column_for_group(g))
            .collect();
        // Include outer collapsed projections and the columns hidden inside
        // them: "all columns" must still apply after the parent expands.
        roots.extend(self.collapsed.iter().map(|c| c.root));
        roots.sort_unstable();
        roots.dedup();
        roots
    }
    pub(crate) fn open_column_group(&self, column: u32) -> Option<u32> {
        let s = self.column_settings.iter().find(|s| s.column == column)?;
        let group = s.open_group?;
        (s.mode == ColumnMode::GroupPanel && self.collapsed_column_for_group(group) == Some(column))
            .then_some(group)
    }
    pub(crate) fn group_panel_width(&self, column: u32) -> f32 {
        self.column_settings
            .iter()
            .find(|s| s.column == column)
            .and_then(|s| s.width)
            .unwrap_or_else(|| {
                self.panels
                    .iter()
                    .filter(|p| {
                        self.panel_group(p.id)
                            .is_some_and(|g| self.node(column).is_some_and(|n| n.find(g).is_some()))
                    })
                    .map(|p| p.id.drawer_width())
                    .fold(TILE_SIZE, f32::max)
            })
    }
    pub(super) fn projected_column_bands(&self, base: &ResolvedLayout) -> Vec<DockBand> {
        fn width(node: &mut DockNode, layout: &DockLayout, base: &ResolvedLayout) -> f32 {
            if layout.is_collapsed(node.id()) {
                return TILE_SIZE
                    + if layout.open_column_group(node.id()).is_some() {
                        layout.group_panel_width(node.id())
                    } else {
                        0.
                    };
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
            if matches!(band.edge, Edge::Left | Edge::Right)
                && self.column_settings.iter().any(|s| {
                    self.open_column_group(s.column).is_some() && band.root.find(s.column).is_some()
                })
            {
                band.extent = width(&mut band.root, self, base) + WORKSPACE_SPACING;
            }
        }
        bands
    }
    pub(super) fn resolve_group_panel(
        &self,
        column: u32,
        bounds: Bounds,
        strip: &mut Bounds,
    ) -> Option<ColumnGroupPanel> {
        let group = self.open_column_group(column)?;
        let panels = self.group_panels(group).ok()?;
        let direction = if self.group_edge(group) == Some(Edge::Right) {
            Edge::Left
        } else {
            Edge::Right
        };
        strip.width = TILE_SIZE.min(bounds.width);
        if direction == Edge::Left {
            strip.x = bounds.x + bounds.width - strip.width;
        }
        let body = Bounds {
            x: if direction == Edge::Right {
                strip.x + strip.width
            } else {
                bounds.x
            },
            width: (bounds.width - strip.width).max(0.),
            ..bounds
        };
        let settings = self.column_settings(column);
        let weights: Vec<_> = panels
            .iter()
            .map(|panel| {
                settings
                    .heights
                    .iter()
                    .find(|h| h.group == group && h.panel == *panel)
                    .map_or(1., |h| h.weight)
            })
            .collect();
        let total: f32 = weights.iter().sum();
        let gap = WORKSPACE_SPACING.min(body.height / (panels.len().max(1) * 2) as f32);
        let usable = (body.height - gap * panels.len().saturating_sub(1) as f32).max(0.);
        let minimum = 36_f32.min(usable / panels.len().max(1) as f32);
        // Preserve ratios except when the window is too short for a panel's
        // minimum; the saved weights are never rewritten by window allocation.
        let mut sizes: Vec<_> = weights.iter().map(|v| usable * v / total).collect();
        let deficit: f32 = sizes.iter().map(|h| (minimum - h).max(0.)).sum();
        let spare: f32 = sizes.iter().map(|h| (h - minimum).max(0.)).sum();
        for h in &mut sizes {
            *h = if *h < minimum {
                minimum
            } else {
                *h - deficit * (*h - minimum) / spare.max(1.)
            };
        }
        let mut y = body.y;
        let mut dividers = Vec::new();
        let panels = panels
            .iter()
            .zip(sizes)
            .enumerate()
            .map(|(i, (panel, height))| {
                if i > 0 {
                    dividers.push(Bounds {
                        y: y - gap,
                        height: gap,
                        ..body
                    });
                }
                let bounds = Bounds { y, height, ..body };
                y += height + gap;
                ColumnIcon {
                    panel: *panel,
                    bounds,
                }
            })
            .collect();
        Some(ColumnGroupPanel {
            group,
            bounds: body,
            panels,
            dividers,
            direction,
            resize: Bounds {
                x: if direction == Edge::Right {
                    body.x + body.width - WORKSPACE_SPACING
                } else {
                    body.x
                },
                width: WORKSPACE_SPACING.min(body.width),
                ..body
            },
        })
    }
    pub(crate) fn resize_column_panel(
        &mut self,
        column: u32,
        after: Option<Panel>,
        position: [f32; 2],
        viewport: [f32; 2],
    ) -> Result<(), String> {
        let resolved = self.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        let p = resolved
            .collapsed
            .iter()
            .find(|c| c.id == column)
            .and_then(|c| c.group_panel.as_ref())
            .ok_or("The group panel is not open")?;
        let settings = self.column_settings_mut(column);
        if let Some(after) = after {
            let i = p
                .panels
                .iter()
                .position(|v| v.panel == after)
                .filter(|i| i + 1 < p.panels.len())
                .ok_or("Unknown panel divider")?;
            let a = p.panels[i].bounds;
            let b = p.panels[i + 1].bounds;
            let total = a.height + b.height;
            let minimum = 36_f32.min(total * 0.5);
            let height =
                (position[1] - a.y - WORKSPACE_SPACING * 0.5).clamp(minimum, total - minimum);
            settings.heights.retain(|h| h.group != p.group);
            for (index, panel) in p.panels.iter().enumerate() {
                settings.heights.push(ColumnPanelHeight {
                    group: p.group,
                    panel: panel.panel,
                    weight: if index == i {
                        height
                    } else if index == i + 1 {
                        total - height
                    } else {
                        panel.bounds.height
                    }
                    .max(0.01),
                });
            }
        } else {
            let width = if p.direction == Edge::Right {
                position[0] - p.bounds.x + WORKSPACE_SPACING * 0.5
            } else {
                p.bounds.x + p.bounds.width - position[0] + WORKSPACE_SPACING * 0.5
            };
            settings.width = Some(width.clamp(128., 800.));
        }
        Ok(())
    }
}
