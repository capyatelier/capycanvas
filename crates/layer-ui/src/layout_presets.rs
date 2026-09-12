//! Shipped arrangements, shared by every host. Hidden panel registrations stay
//! available for drawers and the Window menu; only docked content is visible.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspacePreset {
    Painter,
    Illustrator,
    Photographer,
}

impl WorkspacePreset {
    pub const ALL: [Self; 3] = [Self::Painter, Self::Illustrator, Self::Photographer];

    pub fn name(self) -> &'static str {
        match self {
            Self::Painter => "Painter",
            Self::Illustrator => "Illustrator",
            Self::Photographer => "Photographer",
        }
    }

    /// Initial tools belong to the editable workspace, never the saved layout.
    pub fn working_state(self) -> crate::WorkspaceWorkingState {
        let mut state = crate::WorkspaceWorkingState::default();
        if self != Self::Illustrator {
            state.preset = crate::Tool::Brush.default_preset();
        }
        if self == Self::Photographer {
            state.canvas_tool = crate::LayerCanvasTool::Move;
        }
        state
    }

    pub fn layout(self, platform: crate::Platform) -> DockLayout {
        if self == Self::Illustrator {
            return DockLayout::for_platform(platform);
        }
        use crate::CommandId::*;
        use ToolbarControl::{Color, Divider, Opacity};
        let command = |command| ToolbarControl::Command { command };
        let drawer = |panel| ToolbarControl::Panel { panel };
        let mut layout = DockLayout::editor_default();
        let tools = if self == Self::Painter {
            vec![
                command(Brush),
                command(Eraser),
                command(Blend),
                command(Fill),
                Divider,
                command(Eyedropper),
                Color,
                drawer(Panel::Sizes),
                Opacity,
            ]
        } else {
            vec![
                command(Move),
                command(Lasso),
                command(AutoSelect),
                command(ScaleRotate),
                Divider,
                command(Brush),
                command(Eraser),
                command(Blend),
                command(Liquify),
                command(Fill),
                command(Gradient),
                Divider,
                command(Eyedropper),
                Color,
                command(Hand),
            ]
        };
        let commands = [
            command(Undo),
            command(Redo),
            Divider,
            command(Lasso),
            command(ScaleRotate),
            Divider,
            drawer(Panel::Brushes),
            drawer(Panel::Layers),
        ];
        let photo_commands: Vec<_> = layout
            .panel(Panel::Commands)
            .unwrap()
            .tiles()
            .iter()
            .map(|t| t.control)
            .collect();
        layout.next_tile_id = 1;
        for (panel, controls) in [
            (Panel::Toolbar, tools.as_slice()),
            (
                Panel::Commands,
                if self == Self::Painter {
                    commands.as_slice()
                } else {
                    photo_commands.as_slice()
                },
            ),
        ] {
            let tiles = controls
                .iter()
                .map(|&control| {
                    let id = layout.next_tile_id;
                    layout.next_tile_id += 1;
                    ToolbarTile { id, control }
                })
                .collect();
            let config = layout.panels.iter_mut().find(|p| p.id == panel).unwrap();
            config.hide_tab = true;
            config.tile_style = TileStyle::Medium;
            config.content = PanelContent::Toolbar {
                name: panel.label().into(),
                tiles,
            };
        }
        let tabs = |id, panels: &[Panel]| DockNode::Tabs {
            id,
            panels: panels.to_vec(),
            active: panels[0],
            tab_style: crate::TabStyle::default(),
        };
        let stack = |id, fraction, first, second| DockNode::Split {
            id,
            axis: Axis::Vertical,
            fraction,
            first: Box::new(first),
            second: Box::new(second),
        };
        layout.bands = vec![DockBand {
            id: 1,
            edge: Edge::Left,
            extent: TileStyle::Medium.size()[0] + WORKSPACE_SPACING,
            root: tabs(2, &[Panel::Toolbar]),
        }];
        if self == Self::Painter {
            layout.bands.push(DockBand {
                id: 3,
                edge: Edge::Top,
                extent: TileStyle::Medium.size()[1] + WORKSPACE_SPACING,
                root: tabs(4, &[Panel::Commands]),
            });
            layout.next_id = 5;
        } else {
            // The expanded outer column keeps navigation and layers visible.
            // Secondary controls open inward from a narrow icon column.
            let width = Panel::Layers.default_width() + WORKSPACE_SPACING;
            layout.bands.extend([
                DockBand {
                    id: 3,
                    edge: Edge::Right,
                    extent: width,
                    root: stack(
                        4,
                        0.30,
                        tabs(5, &[Panel::Navigator]),
                        tabs(6, &[Panel::Layers]),
                    ),
                },
                DockBand {
                    id: 7,
                    edge: Edge::Right,
                    extent: TILE_SIZE + WORKSPACE_SPACING,
                    root: stack(
                        8,
                        0.5,
                        tabs(9, &[Panel::Properties, Panel::Adjustments]),
                        tabs(10, &[Panel::Color, Panel::ToolSettings]),
                    ),
                },
            ]);
            layout.collapsed.push(CollapsedColumn {
                root: 8,
                expanded_width: width - WORKSPACE_SPACING,
            });
            layout.next_id = 11;
        }
        layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_portable_and_preserve_illustrator() {
        for platform in [
            crate::Platform::Gtk,
            crate::Platform::Web,
            crate::Platform::Android,
        ] {
            for preset in WorkspacePreset::ALL {
                let layout = preset.layout(platform);
                layout.validate().unwrap();
                crate::WorkspaceCapture {
                    history: crate::LayoutHistory::new(&layout),
                    working: preset.working_state(),
                }
                .validate()
                .unwrap();
                let round_trip: DockLayout =
                    serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
                assert_eq!(round_trip, layout);
            }
            assert_eq!(
                WorkspacePreset::Illustrator.layout(platform),
                DockLayout::for_platform(platform)
            );
        }
    }

    #[test]
    fn painter_has_only_two_medium_toolbars_with_essential_drawers() {
        let layout = WorkspacePreset::Painter.layout(crate::Platform::Gtk);
        assert_eq!(
            layout.bands.iter().map(|b| b.edge).collect::<Vec<_>>(),
            [Edge::Left, Edge::Top]
        );
        assert!(layout.floating.is_empty() && layout.collapsed.is_empty());
        for panel in Panel::ALL {
            assert_eq!(
                layout.panel_group(panel).is_some(),
                matches!(panel, Panel::Toolbar | Panel::Commands)
            );
        }
        for panel in [Panel::Toolbar, Panel::Commands] {
            assert_eq!(layout.panel(panel).unwrap().tile_style, TileStyle::Medium);
        }
        for panel in [Panel::Brushes, Panel::Sizes, Panel::Layers] {
            assert!(
                layout
                    .panels
                    .iter()
                    .flat_map(|p| p.tiles())
                    .any(|t| t.control == ToolbarControl::Panel { panel })
            );
        }
    }

    #[test]
    fn photographer_has_left_tools_and_two_right_columns() {
        let layout = WorkspacePreset::Photographer.layout(crate::Platform::Gtk);
        let resolved = layout.workspace(1600., 1000., crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
        assert_eq!(
            layout.bands.iter().map(|b| b.edge).collect::<Vec<_>>(),
            [Edge::Left, Edge::Right, Edge::Right]
        );
        assert_eq!(layout.collapsed.len(), 1);
        assert!(layout.is_collapsed(8));
        assert!(layout.panel_group(Panel::Layers).is_some());
        assert!(layout.panel_group(Panel::Navigator).is_some());
        for panel in [Panel::Brushes, Panel::Sizes, Panel::Stats, Panel::Commands] {
            assert!(layout.panel_group(panel).is_none());
        }
        assert!(resolved.groups.iter().any(|g| g.active == Panel::Layers));
        let layers = resolved
            .groups
            .iter()
            .find(|g| g.active == Panel::Layers)
            .unwrap();
        let strip = &resolved.collapsed[0];
        assert_eq!(strip.bounds.width, TILE_SIZE);
        assert!(
            (layers.bounds.x - strip.bounds.x - strip.bounds.width - WORKSPACE_SPACING).abs() < 1.
        );
    }
}
