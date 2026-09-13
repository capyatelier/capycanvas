//! Shipped arrangements, shared by every host. Hidden panel registrations stay
//! available for drawers and the Window menu; only docked content is visible.
use super::*;
use crate::HeaderLayout;

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
            Self::Painter => "Paint",
            Self::Illustrator => "Sketch",
            Self::Photographer => "Photo",
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
            let mut layout = DockLayout::for_platform(platform);
            for column in layout.column_roots() {
                layout.column_settings_mut(column).mode = ColumnMode::GroupPanel;
            }
            return layout;
        }
        use crate::CommandId::*;
        use ToolbarControl::{Color, Divider, Opacity};
        let command = |command| ToolbarControl::Command { command };
        let drawer = |panel| ToolbarControl::Panel { panel };
        let mut layout = DockLayout::editor_default();
        layout.header = if self == Self::Painter {
            HeaderLayout::painter_for_platform(platform)
        } else {
            HeaderLayout::for_platform(platform)
        };
        let tile_style = if self == Self::Painter {
            TileStyle::Medium
        } else {
            TileStyle::Small
        };
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
            config.tile_style = tile_style;
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
            extent: tile_style.size()[0] + WORKSPACE_SPACING,
            root: tabs(2, &[Panel::Toolbar]),
        }];
        if self == Self::Painter {
            if platform == crate::Platform::Gtk {
                layout.bands.clear();
                layout.canvas_info.visible = false;
                return layout;
            }
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
    use crate::{HeaderItem, HeaderZone, Platform};

    #[test]
    fn preset_title_bar_controls_follow_the_host_platform() {
        assert_eq!(WorkspacePreset::Illustrator.name(), "Sketch");
        assert_eq!(WorkspacePreset::Painter.name(), "Paint");
        assert_eq!(WorkspacePreset::Photographer.name(), "Photo");
        for platform in [
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Ios,
            Platform::Mac,
            Platform::Windows,
        ] {
            for preset in WorkspacePreset::ALL {
                let layout = preset.layout(platform);
                let items: Vec<_> = layout.header.entries().map(|e| e.item).collect();
                assert!(items.iter().all(|item| item.available_on(platform)));
                assert_eq!(
                    items.contains(&HeaderItem::Fullscreen),
                    platform == Platform::Web
                );
                assert_eq!(
                    layout.header.zones[2].last().unwrap().item,
                    HeaderItem::Settings
                );
                assert!(
                    items.contains(&HeaderItem::Capy) && items.contains(&HeaderItem::Workspaces)
                );
                let minimal = preset == WorkspacePreset::Painter;
                assert_eq!(items.contains(&HeaderItem::Clock), !minimal);
                assert_eq!(items.contains(&HeaderItem::Battery), !minimal);
                assert_eq!(items.contains(&HeaderItem::MenuLabels), !minimal);
                assert_eq!(items.contains(&HeaderItem::Menu), minimal);
                let native = preset.layout(Platform::Gtk).header;
                for zone in HeaderZone::ALL {
                    assert_eq!(
                        layout.header.zones[zone.index()]
                            .iter()
                            .filter(|e| e.item != HeaderItem::Fullscreen)
                            .map(|e| e.item)
                            .collect::<Vec<_>>(),
                        native.zones[zone.index()]
                            .iter()
                            .map(|e| e.item)
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }

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
            let layout = WorkspacePreset::Illustrator.layout(platform);
            assert!(
                layout
                    .column_settings
                    .iter()
                    .all(|s| s.mode == ColumnMode::GroupPanel)
            );
            assert_eq!(layout.bands, DockLayout::for_platform(platform).bands);
        }
    }

    #[test]
    fn painter_has_only_two_medium_toolbars_with_essential_drawers() {
        // Hosts without the new header projection keep their existing controls.
        let layout = WorkspacePreset::Painter.layout(crate::Platform::Web);
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
    fn gtk_painter_has_individual_header_tools_and_no_reserved_status() {
        let layout = WorkspacePreset::Painter.layout(crate::Platform::Gtk);
        assert!(layout.bands.is_empty() && layout.floating.is_empty());
        assert_eq!(layout.header, crate::HeaderLayout::painter());
        assert_eq!(layout.header.size, crate::HeaderSize::Medium);
        assert!(
            !layout
                .header
                .entries()
                .any(|e| e.item == crate::HeaderItem::MenuLabels)
        );
        assert!(!layout.canvas_info.visible);
        assert!(layout.panels.iter().any(|p| p.id == Panel::ToolSettings));
    }

    #[test]
    fn photographer_has_left_tools_and_two_right_columns() {
        let layout = WorkspacePreset::Photographer.layout(crate::Platform::Gtk);
        assert_eq!(
            layout.panel(Panel::Toolbar).unwrap().tile_style,
            TileStyle::Small
        );
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
