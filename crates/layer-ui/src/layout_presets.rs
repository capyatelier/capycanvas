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
            Self::Painter => "Sketch",
            Self::Illustrator => "Paint",
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
        self.layout_with_header_tools(
            platform,
            crate::CommandId::CustomizeWorkspaceUi.available_on(platform),
        )
    }

    /// Exact pre-title-bar arrangement, retained for conservative default upgrades.
    pub fn legacy_painter_layout(platform: crate::Platform) -> DockLayout {
        Self::Painter.layout_with_header_tools(platform, false)
    }

    fn layout_with_header_tools(self, platform: crate::Platform, header_tools: bool) -> DockLayout {
        if self == Self::Illustrator {
            let mut layout = DockLayout::for_platform(platform);
            for column in layout.column_roots() {
                let stack = layout.column_stack_mut(column);
                stack.drawers = false;
                stack.auto_hide = false;
                if matches!(
                    platform,
                    crate::Platform::Gtk
                        | crate::Platform::Web
                        | crate::Platform::Android
                        | crate::Platform::Windows
                ) {
                    let band = layout.bands.iter_mut().find(|b| b.root.id() == column).unwrap();
                    if band.edge != Edge::Right {
                        continue;
                    }
                    layout.collapsed.push(CollapsedColumn {
                        root: column,
                        expanded_width: band.extent - WORKSPACE_SPACING,
                    });
                    band.extent = TILE_SIZE + WORKSPACE_SPACING;
                }
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
            if header_tools {
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

impl DockLayout {
    /// The shipped Paint arrangement opens its right column on adoption or
    /// reset. This is initial presentation; ordinary open/close stays transient.
    pub(crate) fn open_default_columns(&mut self, platform: crate::Platform) {
        if matches!(
            platform,
            crate::Platform::Gtk
                | crate::Platform::Web
                | crate::Platform::Android
                | crate::Platform::Windows
        )
            && self.collapsed.len() == 1
            && crate::durable_layout(self) == WorkspacePreset::Illustrator.layout(platform)
        {
            for stack in &mut self.column_stacks {
                if self.collapsed.iter().any(|c| c.root == stack.column) {
                    stack.open_column = Some(stack.column);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HeaderItem, HeaderZone, Platform};

    #[test]
    fn preset_title_bar_controls_follow_the_host_platform() {
        assert_eq!(WorkspacePreset::Illustrator.name(), "Paint");
        assert_eq!(WorkspacePreset::Painter.name(), "Sketch");
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
                assert!(
                    items.contains(&HeaderItem::Capy) && items.contains(&HeaderItem::Workspaces)
                );
                let minimal = preset == WorkspacePreset::Painter;
                assert_eq!(items.contains(&HeaderItem::Settings), !minimal);
                let last = layout.header.zones[2].last().unwrap().item;
                assert_eq!(
                    last,
                    if !minimal {
                        HeaderItem::Settings
                    } else if platform == Platform::Web {
                        HeaderItem::Fullscreen
                    } else {
                        HeaderItem::Tool { control: ToolbarControl::Color }
                    }
                );
                assert_eq!(items.contains(&HeaderItem::Clock), !minimal);
                assert_eq!(items.contains(&HeaderItem::Battery), !minimal);
                assert_eq!(
                    items.contains(&HeaderItem::MenuLabels),
                    !minimal && platform != Platform::Mac
                );
                assert_eq!(
                    items.contains(&HeaderItem::Menu),
                    minimal && platform != Platform::Mac
                );
                let native = preset.layout(Platform::Gtk).header.projected_for(platform);
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
            crate::Platform::Windows,
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
                    .column_stacks
                    .iter()
                    .all(|s| !s.drawers)
            );
            if matches!(
                platform,
                crate::Platform::Gtk
                    | crate::Platform::Web
                    | crate::Platform::Android
                    | crate::Platform::Windows
            ) {
                assert_eq!(layout.collapsed.len(), 1);
                assert!(layout.is_collapsed(12) && !layout.is_collapsed(4));
                assert!(layout.column_stacks.iter().all(|s| !s.auto_hide && !s.drawers));
                for (band, original) in layout.bands.iter().zip(DockLayout::for_platform(platform).bands) {
                    assert_eq!(band.root, original.root);
                    assert_eq!(band.edge, original.edge);
                    if band.edge == Edge::Left {
                        assert_eq!(band.extent, original.extent);
                    }
                }
            } else {
                assert_eq!(layout.bands, DockLayout::for_platform(platform).bands);
            }
        }
    }

    #[test]
    fn painter_has_only_two_medium_toolbars_with_essential_drawers() {
        // Hosts without the new header projection keep their existing controls.
        let layout = WorkspacePreset::Painter.layout(crate::Platform::Generic);
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
    fn windows_sketch_keeps_tools_accessible_before_header_projection() {
        let layout = WorkspacePreset::Painter.layout(crate::Platform::Windows);
        assert!(layout.panel_group(Panel::Toolbar).is_some());
        assert!(layout.panel_group(Panel::Commands).is_some());
        assert!(layout.canvas_info.visible);
    }

    #[test]
    fn projected_sketch_has_only_individual_header_tools() {
        for platform in [
            crate::Platform::Gtk,
            crate::Platform::Web,
            crate::Platform::Android,
            crate::Platform::Ios,
            crate::Platform::Mac,
        ] {
            let layout = WorkspacePreset::Painter.layout(platform);
            assert!(layout.bands.is_empty() && layout.floating.is_empty());
            assert_eq!(
                layout.header,
                crate::HeaderLayout::painter_for_platform(platform)
            );
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
