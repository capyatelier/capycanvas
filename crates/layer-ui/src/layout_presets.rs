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
        let mut layout = self.legacy_bottom_brush_controls_layout(platform);
        if self == Self::Painter && platform == crate::Platform::Gtk {
            let panel = layout.panels.iter().find(|p| {
                p.tiles().iter().any(|t| t.control == ToolbarControl::BrushSizeSlider)
            }).unwrap().id;
            layout.move_panel(
                [1600., 1000.],
                panel,
                DockTarget::CompactEdge { edge: Edge::Left, alignment: EdgeAlignment::Center },
            ).expect("centered brush toolbar");
        }
        if self == Self::Photographer && platform == crate::Platform::Gtk {
            layout.move_panel([1600., 1000.], Panel::Commands,
                DockTarget::Edge { edge: Edge::Top, outer: true })
                .expect("outer Photo options bar");
        }
        layout
    }

    /// Previous component default; only untouched included layouts migrate.
    pub fn legacy_bottom_brush_controls_layout(self, platform: crate::Platform) -> DockLayout {
        let mut layout = self.legacy_toolbar_components_layout(platform);
        if platform == crate::Platform::Gtk {
            match self {
                Self::Painter => {
                    let panel = layout
                        .add_toolbar(
                            None,
                            "Brush controls",
                            &[
                                ToolbarControl::BrushSizeSlider,
                                ToolbarControl::BrushOpacitySlider,
                            ],
                        )
                        .expect("built-in brush controls");
                    layout
                        .panels
                        .iter_mut()
                        .find(|p| p.id == panel)
                        .unwrap()
                        .tile_style = TileStyle::Medium;
                    layout
                        .move_panel(
                            [1600., 1000.],
                            panel,
                            DockTarget::Edge {
                                edge: Edge::Bottom,
                                outer: false,
                            },
                        )
                        .expect("bottom brush toolbar");
                }
                Self::Photographer => {
                    let config = layout
                        .panels
                        .iter_mut()
                        .find(|p| p.id == Panel::Commands)
                        .unwrap();
                    config.tiles_mut().unwrap().retain(|t| {
                        !matches!(
                            t.control,
                            ToolbarControl::Command {
                                command: crate::CommandId::ClearLayer
                                    | crate::CommandId::FillSelection
                            }
                        )
                    });
                    layout
                        .insert_tools(
                            Panel::Commands,
                            None,
                            &[ToolbarControl::Divider, ToolbarControl::TOOL_OPTIONS],
                        )
                        .unwrap();
                }
                Self::Illustrator => (),
            }
        }
        layout
    }


    /// Exact defaults before inline GTK toolbar components; migrate untouched
    /// included workspaces without replacing any customized arrangement.
    pub fn legacy_toolbar_components_layout(self, platform: crate::Platform) -> DockLayout {
        let mut layout = self.legacy_selection_layout(platform);
        if crate::CommandId::Select.available_on(platform) {
            if self == Self::Painter {
                replace_tool(&mut layout, crate::CommandId::Lasso, crate::CommandId::Select);
            } else if self == Self::Photographer {
                use crate::CommandId::*;
                for (before_command, commands) in [(Lasso, &[RectangleSelect, EllipseSelect][..]), (AutoSelect, &[PolygonSelect][..]), (Fill, &[ColorSelect][..])] {
                    let before = layout.panel(Panel::Toolbar).unwrap().tiles().iter()
                        .find(|tile| tile.control == ToolbarControl::Command { command: before_command }).map(|tile| tile.id);
                    let controls: Vec<_> = commands.iter().map(|&command| ToolbarControl::Command { command }).collect();
                    layout.insert_tools(Panel::Toolbar, before, &controls).expect("included selection tools");
                }
            }
        }
        layout
    }

    /// Exact first GTK component default, used only to upgrade untouched saves.
    pub fn legacy_brush_controls_layout(platform: crate::Platform) -> DockLayout {
        let mut layout = Self::Painter.legacy_toolbar_components_layout(platform);
        if platform == crate::Platform::Gtk {
            let config = layout
                .panels
                .iter_mut()
                .find(|p| p.id == Panel::Commands)
                .unwrap();
            config.content = PanelContent::Toolbar {
                name: "Brush controls".into(),
                tiles: Vec::new(),
            };
            layout
                .insert_tools(
                    Panel::Commands,
                    None,
                    &[
                        ToolbarControl::BrushSizeSlider,
                        ToolbarControl::BrushOpacitySlider,
                    ],
                )
                .unwrap();
            let id = layout.next_id;
            layout.next_id += 2;
            layout.bands.push(DockBand {
                alignment: None,
                id,
                edge: Edge::Bottom,
                extent: TileStyle::Medium.size()[1] + WORKSPACE_SPACING,
                root: DockNode::Tabs {
                    id: id + 1,
                    panels: vec![Panel::Commands],
                    active: Panel::Commands,
                    tab_style: crate::TabStyle::default(),
                },
            });
        }
        layout
    }


    /// Last shipped defaults before the selection family rollout.
    pub fn legacy_selection_layout(self, platform: crate::Platform) -> DockLayout {
        let mut layout=if self == Self::Photographer && platform != crate::Platform::Generic {
            Self::legacy_illustrator_primary_layout(platform)
        } else {
            self.layout_with_header_tools(platform,crate::CommandId::CustomizeWorkspaceUi.available_on(platform))
        };
        if self == Self::Painter && crate::CommandId::DrawingBrush.available_on(platform) {
            for (old, new) in [(crate::CommandId::Brush, crate::CommandId::DrawingBrush), (crate::CommandId::Blend, crate::CommandId::Sculpt)] {
                replace_tool(&mut layout, old, new);
            }
        }

        if self != Self::Painter && Panel::Proof.available_on(platform) {
            if let Some(group)=layout.panel_group(Panel::Color) {
                if let Some(DockNode::Tabs {panels,..})=layout.node_mut(group) {
                    let index=panels.iter().position(|p| *p==Panel::Color).unwrap()+1;
                    panels.insert(index,Panel::Proof);
                }
            }
        }
        layout
    }

    /// Exact pre-title-bar arrangement, retained for conservative default upgrades.
    pub fn legacy_painter_layout(platform: crate::Platform) -> DockLayout {
        Self::Painter.layout_with_header_tools(platform, false)
    }

    /// The three-panel Brush drawer replaces the prior Paint button.
    pub fn legacy_painter_paint_drawer_layout(platform: crate::Platform) -> DockLayout {
        Self::Painter.layout_with_header_tools(platform, crate::CommandId::CustomizeWorkspaceUi.available_on(platform))
    }

    /// Exact prior Photo columns, before adopting the shared primary panels.
    pub fn legacy_photographer_layout(platform: crate::Platform) -> DockLayout {
        Self::Photographer.layout_with_header_tools(platform, false)
    }

    /// Exact prior Paint arrangement, retained for conservative default upgrades.
    pub fn legacy_illustrator_layout(platform: crate::Platform) -> DockLayout {
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
                    | crate::Platform::Mac
                    | crate::Platform::Ios
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
        layout
    }

    /// Temporary Paint arrangement now used by Photo. Retained so untouched
    /// Paint workspaces from that version can return to their original default.
    pub fn legacy_illustrator_primary_layout(platform: crate::Platform) -> DockLayout {
        let mut layout = Self::legacy_illustrator_layout(platform);
        if !matches!(platform, crate::Platform::Generic) {
            // Keep the primary column permanently expanded at the outer
            // right edge. Secondary panels occupy the icon strip inward
            // from it, in Tool Set / Tool + Brush size / Navigator order.
            if let Some(DockNode::Tabs { panels, active, .. }) = layout.node_mut(14) {
                *panels = vec![Panel::Color, Panel::Stats];
                *active = Panel::Color;
            }
            if let Some(DockNode::Tabs { panels, active, .. }) = layout.node_mut(10) {
                *panels = vec![Panel::Navigator];
                *active = Panel::Navigator;
            }
            let mut secondary = layout.bands.remove(1);
            secondary.edge = Edge::Right;
            let expanded_width = secondary.extent - WORKSPACE_SPACING;
            secondary.extent = TILE_SIZE + WORKSPACE_SPACING;
            layout.bands[1].extent = Panel::Layers.default_width() + WORKSPACE_SPACING;
            layout.bands.insert(2, secondary);
            layout.collapsed = vec![CollapsedColumn { root: 4, expanded_width }];
        }
        layout
    }

    fn layout_with_header_tools(self, platform: crate::Platform, header_tools: bool) -> DockLayout {
        if self == Self::Illustrator {
            return Self::legacy_illustrator_layout(platform);
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
        // This builder also supplies the exact previous defaults for migration.
        // The current layout promotes Paint Brush and Blend to their tool classes.
        layout.header.replace_tool(DrawingBrush, Brush);
        layout.header.replace_tool(Select, Lasso);
        layout.header.replace_tool(Sculpt, Blend);
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
            alignment: None,
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
                alignment: None,
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
                    alignment: None,
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
                    alignment: None,
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
    /// Paint opens its original right stack on adoption, preview and reset.
    /// Ordinary open/close remains transient; Photo uses a fixed outer column.
    pub(crate) fn open_default_columns(&mut self, platform: crate::Platform) {
        if matches!(
            platform,
            crate::Platform::Gtk
                | crate::Platform::Web
                | crate::Platform::Android
                | crate::Platform::Windows
                | crate::Platform::Mac
                | crate::Platform::Ios
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

fn replace_tool(layout: &mut DockLayout, old: crate::CommandId, new: crate::CommandId) {
    layout.header.replace_tool(old, new);
    let old = ToolbarControl::Command { command: old };
    let new = ToolbarControl::Command { command: new };
    for panel in &mut layout.panels {
        if let PanelContent::Toolbar { tiles, .. } = &mut panel.content {
            for tile in tiles {
                if tile.control == old { tile.control = new; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HeaderItem, HeaderZone, Platform};

    #[test]
    fn tool_class_panels_are_independent_and_available_on_supported_hosts() {
        let gtk = WorkspacePreset::Painter.layout(Platform::Gtk);
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows, Platform::Mac, Platform::Ios] {
            let layout = WorkspacePreset::Painter.layout(platform);
            for panel in [Panel::BrushSets, Panel::SculptSets, Panel::Tools, Panel::FilterTypes] {
                assert!(layout.panel(panel).is_ok());
                assert!(layout.panel_group(panel).is_none());
                assert!(panel.available_on(platform));
            }
            assert!(crate::CommandId::DrawingBrush.available_on(platform));
            assert!(crate::CommandId::Sculpt.available_on(platform));
        }
        assert!(Panel::BrushSets.default_width() < Panel::Tools.default_width());
        assert_eq!(Panel::BrushSets.label(), "Brushes");
        assert_eq!(Panel::Tools.label(), "Tools");
        // Existing workspaces gain hidden registrations without losing layout.
        let mut saved = serde_json::to_value(&gtk).unwrap();
        saved["panels"].as_array_mut().unwrap().retain(|panel|
            panel["id"] != "brush_sets" && panel["id"] != "tools" && panel["id"] != "sculpt_sets");
        let restored: DockLayout = serde_json::from_value(saved).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.header, gtk.header);
        assert_eq!(restored.bands, gtk.bands);
        for panel in &gtk.panels {
            assert_eq!(restored.panel(panel.id).unwrap(), panel);
        }
    }

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
                let mut native = preset.layout(Platform::Gtk).header.projected_for(platform);
                if !crate::CommandId::Select.available_on(platform) { native.replace_tool(crate::CommandId::Select, crate::CommandId::Lasso); }
                // Hosts without these projections retain the prior tools.
                if !crate::CommandId::DrawingBrush.available_on(platform) {
                    native.replace_tool(crate::CommandId::DrawingBrush, crate::CommandId::Brush);
                    native.replace_tool(crate::CommandId::Sculpt, crate::CommandId::Blend);
                }
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
    fn presets_are_portable_and_photo_keeps_primary_panels_at_the_right() {
        for platform in [
            crate::Platform::Gtk,
            crate::Platform::Web,
            crate::Platform::Android,
            crate::Platform::Windows,
            crate::Platform::Mac,
            crate::Platform::Ios,
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
            let mut layout = WorkspacePreset::Photographer.layout(platform);
            assert!(
                layout
                    .column_stacks
                    .iter()
                    .all(|s| !s.drawers)
            );
            assert_eq!(layout.collapsed.len(), 1);
            assert!(layout.is_collapsed(4) && !layout.is_collapsed(12));
            assert!(layout.column_stacks.iter().all(|s| !s.auto_hide && !s.drawers));
            layout.open_default_columns(platform);
            assert!(layout.column_stacks.iter().all(|s| s.open_column.is_none()));
            assert_eq!(layout.bands.iter().map(|b| b.edge).collect::<Vec<_>>(),
                if platform == crate::Platform::Gtk { [Edge::Top, Edge::Left, Edge::Right, Edge::Right] }
                else { [Edge::Left, Edge::Right, Edge::Right, Edge::Top] });
            for (id, expected) in [
                (14, if Panel::Proof.available_on(platform) {vec![Panel::Color, Panel::Proof, Panel::Stats]} else {vec![Panel::Color, Panel::Stats]}),
                (15, vec![Panel::Properties, Panel::Adjustments]),
                (16, vec![Panel::Layers]),
                (6, vec![Panel::Brushes]),
                (7, vec![Panel::ToolSettings, Panel::Sizes]),
                (10, vec![Panel::Navigator]),
            ] {
                let DockNode::Tabs { panels, active, .. } = layout.node(id).unwrap() else {
                    panic!("default tab group");
                };
                assert_eq!(panels, &expected);
                assert_eq!(*active, expected[0]);
            }
            for [width, height] in [[1600., 1200.], [1200., 800.], [640., 480.]] {
                let resolved = layout.workspace(width, height, crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
                let group = |panel| resolved.groups.iter().find(|g| g.panels.contains(&panel)).unwrap().bounds;
                let color = group(Panel::Color);
                let properties = group(Panel::Properties);
                let layers = group(Panel::Layers);
                assert_eq!(color, group(Panel::Stats));
                assert_eq!(properties, group(Panel::Adjustments));
                assert_eq!(color.x, properties.x);
                assert_eq!(properties.x, layers.x);
                assert!(color.y + color.height < properties.y);
                assert!(properties.y + properties.height < layers.y);
                let strip = &resolved.collapsed[0];
                assert!((strip.bounds.x + strip.bounds.width + WORKSPACE_SPACING - color.x).abs() < 1.);
                assert!(resolved.work_area.width > 0. && resolved.work_area.height > 0.);
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
    fn generic_sketch_keeps_tools_accessible_without_header_projection() {
        let layout = WorkspacePreset::Painter.layout(crate::Platform::Generic);
        assert!(layout.panel_group(Panel::Toolbar).is_some());
        assert!(layout.panel_group(Panel::Commands).is_some());
        assert!(layout.canvas_info.visible);
    }

    #[test]
    fn projected_sketch_keeps_header_tools_and_gtk_brush_sliders() {
        for platform in [
            crate::Platform::Gtk,
            crate::Platform::Web,
            crate::Platform::Android,
            crate::Platform::Ios,
            crate::Platform::Mac,
            crate::Platform::Windows,
        ] {
            let layout = WorkspacePreset::Painter.layout(platform);
            assert!(layout.floating.is_empty());
            if platform == crate::Platform::Gtk {
                assert_eq!(layout.bands.len(), 1);
                assert_eq!(layout.bands[0].edge, Edge::Left);
                assert_eq!(layout.bands[0].alignment, Some(EdgeAlignment::Center));
            } else { assert!(layout.bands.is_empty()); }
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
    fn photo_adopts_the_reviewed_layout_and_paint_restores_its_original_default() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows, Platform::Mac, Platform::Ios] {
            for (preset,mut previous) in [(WorkspacePreset::Photographer,WorkspacePreset::legacy_illustrator_primary_layout(platform)),(WorkspacePreset::Illustrator,WorkspacePreset::legacy_illustrator_layout(platform))] {
                let mut current=preset.legacy_toolbar_components_layout(platform);
                if preset == WorkspacePreset::Photographer && crate::CommandId::Select.available_on(platform) {
                    use crate::CommandId::*;
                    current.panels.iter_mut().find(|p| p.id == Panel::Toolbar).unwrap().tiles_mut().unwrap().retain(|t|
                        !matches!(t.control, ToolbarControl::Command { command: RectangleSelect | EllipseSelect | PolygonSelect | ColorSelect }));
                    current.next_tile_id = previous.next_tile_id;
                }
                if Panel::Proof.available_on(platform) {
                    assert_eq!(current.panel_group(Panel::Proof),current.panel_group(Panel::Color));
                    let group=previous.panel_group(Panel::Color).unwrap();
                    let DockNode::Tabs {panels,..}=previous.node_mut(group).unwrap() else {panic!()};
                    panels.insert(1,Panel::Proof);
                }
                assert_eq!(current,previous);
            }
            assert_eq!(WorkspacePreset::Photographer.working_state().canvas_tool, crate::LayerCanvasTool::Move);
        }
    }
}
