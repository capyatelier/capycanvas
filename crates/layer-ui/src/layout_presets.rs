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

    pub fn layout(self, platform: crate::Platform) -> DockLayout {
        let mut layout = match self {
            Self::Painter => return Self::painter_layout(platform),
            Self::Illustrator => {
                let mut layout = Self::columns_layout(platform);
                insert_proof(&mut layout);
                fit_paint_columns(&mut layout);
                layout
            }
            Self::Photographer => Self::photo_layout(platform),
        };
        for (panel, anchor) in [
            (Panel::Palettes, Panel::Color),
            (Panel::Proof, Panel::Navigator),
            (Panel::Stats, Panel::Brushes),
        ] {
            if let Some(group) = layout.panel_group(anchor) {
                let index = layout
                    .group_panels(group)
                    .unwrap()
                    .iter()
                    .position(|p| *p == anchor)
                    .unwrap()
                    + 1;
                layout
                    .set_panel_visible(panel, true)
                    .expect("registered default panel");
                layout
                    .move_panel(
                        [1600., 1000.],
                        panel,
                        DockTarget::Tab {
                            group,
                            index: Some(index),
                        },
                    )
                    .expect("default tab order");
                layout
                    .select_tab(group, anchor)
                    .expect("default active tab");
            }
        }
        self.group_tools(&mut layout);
        let group = layout.panel_group(Panel::Color).unwrap();
        if !layout.fit_height_groups.contains(&group) {
            layout.fit_height_groups.push(group);
        }
        if self == Self::Photographer {
            if let Some(DockNode::Tabs {panels,active,..})=layout.node_mut(group) {
                *panels=vec![Panel::Histogram,Panel::Waveform];
                *active=Panel::Histogram;
            }
            fit_right_column(&mut layout, 0.6);
        }
        layout
    }

    fn group_tools(self, layout: &mut DockLayout) {
        use crate::{CommandId as C, ToolSlotId as S};
        use ToolbarControl::{Command, ToolSlot, Divider, Color};
        if self == Self::Photographer {
            let controls = [
                ToolSlot {slot:S::Operation}, Command {command:C::Crop}, Divider,
                ToolSlot {slot:S::Marquee}, ToolSlot {slot:S::Lasso}, ToolSlot {slot:S::AutomaticSelection}, Command {command:C::SelectionBrush}, Divider,
                ToolSlot {slot:S::Drawing}, Command {command:C::Eraser}, Command {command:C::Clone}, ToolSlot {slot:S::Healing},
                Command {command:C::Blend}, Command {command:C::Liquify}, ToolSlot {slot:S::PhotoFill}, Divider,
                Command {command:C::Hand}, Command {command:C::Eyedropper}, Color,
            ];
            let tiles=controls.into_iter().map(|control| {
                let id=layout.next_tile_id;
                layout.next_tile_id+=1;
                ToolbarTile {id,control}
            }).collect();
            *layout.panel_mut(Panel::Toolbar).unwrap().tiles_mut().unwrap()=tiles;
        } else if self == Self::Illustrator {
            for tile in layout.panel_mut(Panel::Toolbar).unwrap().tiles_mut().unwrap() {
                let Command {command}=tile.control else {continue;};
                let slot=match command {
                    C::Blend=>S::Blend, C::Lasso=>S::ManualSelection, C::AutoSelect=>S::AutomaticSelection,
                    C::Fill=>S::Fill, C::Gradient=>S::Gradient, C::Move=>S::Operation, C::Figure=>S::Figure, C::Ruler=>S::Ruler,
                    _=>continue,
                };
                tile.control=ToolSlot {slot};
            }
        }
    }

    fn painter_layout(platform: crate::Platform) -> DockLayout {
        use crate::CommandId::*;
        use ToolbarControl::{
            BrushOpacitySlider, BrushSizeSlider, Color, ColorPicker, Divider, Opacity,
        };
        let command = |command| ToolbarControl::Command { command };
        let drawer = |panel| ToolbarControl::Panel { panel };
        let mut layout = DockLayout::editor_default();
        layout.header = HeaderLayout::painter_for_platform(platform);
        layout.bands.clear();
        layout.canvas_info.visible = false;
        layout.next_tile_id = 1;
        for (panel, controls) in [
            (
                Panel::Toolbar,
                &[
                    command(DrawingBrush),
                    command(Eraser),
                    command(Sculpt),
                    command(Fill),
                    Divider,
                    command(Eyedropper),
                    Color,
                    drawer(Panel::Sizes),
                    Opacity,
                ][..],
            ),
            (
                Panel::Commands,
                &[
                    command(Undo),
                    command(Redo),
                    Divider,
                    command(Select),
                    command(ScaleRotate),
                    Divider,
                    drawer(Panel::Brushes),
                    drawer(Panel::Layers),
                ],
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
                name: None,
                tiles,
            };
        }
        let panel = layout
            .add_toolbar(None, "Brush controls", &[BrushSizeSlider, BrushOpacitySlider])
            .expect("built-in brush controls");
        let config = layout.panels.iter_mut().find(|p| p.id == panel).unwrap();
        config.tile_style = TileStyle::Medium;
        let opacity = config
            .tiles()
            .iter()
            .find(|t| t.control == BrushOpacitySlider)
            .unwrap()
            .id;
        layout
            .move_panel(
                [1600., 1000.],
                panel,
                DockTarget::CompactEdge {
                    edge: Edge::Left,
                    alignment: EdgeAlignment::Center,
                },
            )
            .expect("centered brush toolbar");
        layout
            .insert_tools(panel, Some(opacity), &[ColorPicker])
            .expect("Sketch color picker");
        layout
            .insert_tools(panel, None, &[command(Undo), command(Redo)])
            .expect("Sketch history buttons");
        layout
    }

    fn photo_layout(platform: crate::Platform) -> DockLayout {
        use crate::CommandId::*;
        let mut layout = Self::columns_layout(platform);
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
        layout.collapsed = vec![CollapsedColumn {
            root: 4,
            expanded_width,
        }];
        insert_proof(&mut layout);
        let tiles = layout.panel(Panel::Toolbar).unwrap().tiles();
        let after_liquify = tiles
            .iter()
            .position(|tile| tile.control == ToolbarControl::Command { command: Liquify })
            .and_then(|index| tiles.get(index + 1))
            .map(|tile| tile.id);
        layout
            .insert_tools(
                Panel::Toolbar,
                after_liquify,
                &[crate::CommandId::Clone, crate::CommandId::Heal, crate::CommandId::SpotHeal]
                    .map(|command| ToolbarControl::Command { command }),
            )
            .expect("retouching tools after the painting tools");
        for (before, commands) in [
            (Lasso, &[RectangleSelect, EllipseSelect][..]),
            (AutoSelect, &[PolygonSelect][..]),
            (Fill, &[ColorSelect][..]),
            (Figure, &[Crop][..]),
        ] {
            let before = layout
                .panel(Panel::Toolbar)
                .unwrap()
                .tiles()
                .iter()
                .find(|tile| tile.control == ToolbarControl::Command { command: before })
                .map(|tile| tile.id);
            let controls: Vec<_> = commands
                .iter()
                .map(|&command| ToolbarControl::Command { command })
                .collect();
            layout
                .insert_tools(Panel::Toolbar, before, &controls)
                .expect("included selection tools");
        }
        layout
            .panel_mut(Panel::Commands)
            .unwrap()
            .tiles_mut()
            .unwrap()
            .retain(|t| {
                !matches!(
                    t.control,
                    ToolbarControl::Command {
                        command: ClearLayer | FillSelection | FlipHorizontal
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
        layout
            .move_panel(
                [1600., 1000.],
                Panel::Commands,
                DockTarget::Edge {
                    edge: Edge::Top,
                    outer: true,
                },
            )
            .expect("outer Photo options bar");
        layout.column_stack_mut(4).drawers = true;
        layout
    }

    fn columns_layout(platform: crate::Platform) -> DockLayout {
        let mut layout = DockLayout::for_platform(platform);
        for column in layout.column_roots() {
            let stack = layout.column_stack_mut(column);
            stack.drawers = false;
            stack.auto_hide = false;
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
        layout
    }
}

fn insert_proof(layout: &mut DockLayout) {
    if let Some(group) = layout.panel_group(Panel::Color)
        && let Some(DockNode::Tabs { panels, .. }) = layout.node_mut(group)
    {
        let index = panels.iter().position(|p| *p == Panel::Color).unwrap() + 1;
        panels.insert(index, Panel::Proof);
    }
}

impl DockLayout {
    /// Paint opens its original right stack on adoption, preview and reset.
    /// Ordinary open/close remains transient; Photo uses a fixed outer column.
    pub(crate) fn open_default_columns(&mut self, platform: crate::Platform) {
        if self.collapsed.len() == 1
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

fn fit_paint_columns(layout: &mut DockLayout) {
    let group = |id| layout.node(id).cloned().expect("Paint default group");
    let stack = |id, fraction, first, second| DockNode::Split {
        id,
        axis: Axis::Vertical,
        fraction,
        first: Box::new(first),
        second: Box::new(second),
    };
    let left = stack(4, 0.6528, stack(5, 0.5, group(6), group(7)), group(10));
    layout.bands.iter_mut().find(|b| b.id == 3).expect("Paint default column").root = left;
    fit_right_column(layout, 0.4);
    layout.fit_height_groups = vec![10, 14];
}

fn fit_right_column(layout: &mut DockLayout, properties_fraction: f32) {
    let group = |id| Box::new(layout.node(id).cloned().expect("default primary panel"));
    let root = DockNode::Split {id:12,axis:Axis::Vertical,fraction:0.25,first:group(14),
        second:Box::new(DockNode::Split {id:13,axis:Axis::Vertical,fraction:properties_fraction,first:group(15),second:group(16)})};
    layout.bands.iter_mut().find(|b| b.id == 11).expect("default primary column").root = root;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HeaderItem, HeaderZone, Platform};

    #[test]
    fn tool_class_panels_are_independent_and_available_on_supported_hosts() {
        for platform in Platform::ALL {
            let layout = WorkspacePreset::Painter.layout(platform);
            for panel in [Panel::BrushSets, Panel::SculptSets, Panel::Tools, Panel::FilterTypes] {
                assert!(layout.panel(panel).is_ok());
                assert!(layout.panel_group(panel).is_none());
            }
            assert!(crate::CommandId::DrawingBrush.available_on(platform));
            assert!(crate::CommandId::Sculpt.available_on(platform));
        }
        assert!(Panel::BrushSets.default_width() < Panel::Tools.default_width());
        assert_eq!(Panel::BrushSets.canonical_label().as_ref(), "Brushes");
        assert_eq!(Panel::Tools.canonical_label().as_ref(), "Tools");
    }

    #[test]
    fn preset_title_bar_controls_follow_the_host_platform() {
        assert_eq!(WorkspacePreset::Illustrator.name(), "Paint");
        assert_eq!(WorkspacePreset::Painter.name(), "Sketch");
        assert_eq!(WorkspacePreset::Photographer.name(), "Photo");
        for platform in Platform::ALL {
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
                    working: crate::WorkspaceWorkingState::default(),
                }
                .validate_structure()
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
                    .all(|s| s.drawers == (s.column == 4))
            );
            assert_eq!(layout.collapsed.len(), 1);
            assert!(layout.is_collapsed(4) && !layout.is_collapsed(12));
            assert!(layout.column_stacks.iter().all(|s| !s.auto_hide));
            assert!(layout.column_stack(4).drawers);
            layout.open_default_columns(platform);
            assert!(layout.column_stacks.iter().all(|s| s.open_column.is_none()));
            assert_eq!(layout.bands.iter().map(|b| b.edge).collect::<Vec<_>>(),
                [Edge::Top, Edge::Left, Edge::Right, Edge::Right]);
            for (id, expected) in [
                (14, vec![Panel::Histogram,Panel::Waveform]),
                (15, vec![Panel::Properties, Panel::Adjustments]),
                (16, vec![Panel::Layers]),
                (6, vec![Panel::Brushes, Panel::Stats]),
                (7, vec![Panel::ToolSettings, Panel::Sizes]),
                (10, vec![Panel::Navigator, Panel::Proof]),
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
                let color = group(Panel::Histogram);
                let properties = group(Panel::Properties);
                let layers = group(Panel::Layers);
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
    fn projected_sketch_keeps_header_tools_and_supported_brush_sliders() {
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
            assert_eq!(layout.bands.len(), 1);
            assert_eq!(layout.bands[0].edge, Edge::Left);
            assert_eq!(layout.bands[0].alignment, Some(EdgeAlignment::Center));
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
    fn palette_defaults_are_adjacent_bounded_and_portable() {
        for (preset, platform) in [WorkspacePreset::Illustrator, WorkspacePreset::Photographer]
            .into_iter()
            .flat_map(|preset| {
                [
                    crate::Platform::Gtk,
                    crate::Platform::Web,
                    crate::Platform::Android,
                    crate::Platform::Mac,
                    crate::Platform::Ios,
                    crate::Platform::Windows,
                ]
                .map(|platform| (preset, platform))
            })
        {
            let mut layout = preset.layout(platform);
            layout.open_default_columns(platform);
            layout.validate().unwrap();
            let scopes=preset==WorkspacePreset::Photographer;
            let (anchor,selected,minimum_width)=if scopes {
                for panel in [Panel::Color,Panel::Palettes] {assert!(layout.panel(panel).is_ok());assert!(layout.panel_group(panel).is_none());}
                (Panel::Histogram,Panel::Waveform,254.)
            } else {(Panel::Color,Panel::Palettes,280.)};
            for (panel, anchor) in [
                (selected, anchor),
                (Panel::Proof, Panel::Navigator),
                (Panel::Stats, Panel::Brushes),
            ] {
                let group = layout.panel_group(anchor).unwrap();
                let panels = layout.group_panels(group).unwrap();
                let index = panels.iter().position(|p| *p == anchor).unwrap();
                assert_eq!(panels[index + 1], panel);
                assert_eq!(layout.active_panel(panel), Some(anchor));
            }
            let group = layout.panel_group(anchor).unwrap();
            layout.select_tab(group, selected).unwrap();
            let colors = layout
                .resolve(1600., 1000.)
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds;
            assert!(colors.width >= minimum_width);
            let restored: DockLayout =
                serde_json::from_slice(&serde_json::to_vec(&layout).unwrap()).unwrap();
            assert_eq!(
                crate::durable_layout(&restored),
                crate::durable_layout(&layout)
            );
        }
    }

    #[test]
    fn fitted_groups_keep_their_bounds_when_switching_pages() {
        for preset in [WorkspacePreset::Illustrator, WorkspacePreset::Photographer] {
            let mut layout = preset.layout(Platform::Gtk);
            layout.open_default_columns(Platform::Gtk);
            let [first,second]=if preset==WorkspacePreset::Photographer {[Panel::Histogram,Panel::Waveform]}else{[Panel::Color,Panel::Palettes]};
            layout.measurements = [(first, 300.), (second, 200.)]
                .map(|(panel, content_height)| PanelMeasurement {
                    panel,
                    tab_width: 0.,
                    content_height,
                    scroll: None,
                })
                .to_vec();
            let group = layout.panel_group(first).unwrap();
            let bounds = |l: &DockLayout| {
                l.resolve(1400., 1000.)
                    .groups
                    .into_iter()
                    .find(|g| g.id == group)
                    .unwrap()
                    .bounds
            };
            let fitted = bounds(&layout);
            assert_eq!(fitted.height, 300. + TAB_BAR_HEIGHT);
            for panel in [second, first, second] {
                layout.select_tab(group, panel).unwrap();
                assert_eq!(bounds(&layout), fitted);
            }
            // Height fitting responds to native width/font measurements even
            // while another page is selected; it needs no remembered tab state.
            layout.measurements[0].content_height = 340.;
            assert_eq!(bounds(&layout).height, 340. + TAB_BAR_HEIGHT);
            layout.add_panel_to_group(Panel::Layers, group).unwrap();
            layout.measurements.push(PanelMeasurement {
                panel: Panel::Layers,
                tab_width: 0.,
                content_height: 10_000.,
                scroll: Some(PanelScrollMeasurement {
                    fixed_height: 40.,
                    unit_height: 48.,
                }),
            });
            assert_eq!(
                bounds(&layout).height,
                340. + TAB_BAR_HEIGHT,
                "scrolling rows do not inflate the group"
            );
        }
    }

    #[test]
    fn photo_scopes_fit_natural_height_and_properties_receive_sixty_percent_of_remainder() {
        let mut layout=WorkspacePreset::Photographer.layout(Platform::Gtk);layout.open_default_columns(Platform::Gtk);
        layout.measurements=[
            PanelMeasurement{panel:Panel::Histogram,content_height:265.,tab_width:111.,scroll:None},
            PanelMeasurement{panel:Panel::Waveform,content_height:241.,tab_width:110.,scroll:None},
            PanelMeasurement{panel:Panel::Properties,content_height:99.,tab_width:111.,scroll:Some(PanelScrollMeasurement{fixed_height:0.,unit_height:0.})},
            PanelMeasurement{panel:Panel::Layers,content_height:170.,tab_width:85.,scroll:Some(PanelScrollMeasurement{fixed_height:90.,unit_height:40.})},
        ].to_vec();
        for width in [640.,1100.] {
            let resolved=layout.workspace(width,800.,crate::HEADER_HEIGHT,crate::STATUS_HEIGHT);
            let bounds=|panel|resolved.groups.iter().find(|g|g.panels.contains(&panel)).unwrap().bounds;
            let scopes=bounds(Panel::Histogram);let properties=bounds(Panel::Properties);let layers=bounds(Panel::Layers);
            assert_eq!(scopes.height,265.+TAB_BAR_HEIGHT);assert_eq!(scopes,bounds(Panel::Waveform));
            assert!((properties.height/(properties.height+layers.height)-0.6).abs()<0.01,"width {width}: {properties:?} {layers:?}");
            assert!(properties.height>200.,"width {width}: {properties:?}");assert!(properties.height-TAB_BAR_HEIGHT>=99.);assert!(scopes.y+scopes.height<properties.y);assert!(properties.y+properties.height<layers.y);assert_eq!(properties.x,layers.x);
        }
    }

    #[test]
    fn paint_color_and_navigator_fit_their_content_and_share_the_rest() {
        let bounds = |layout: &DockLayout, height, panel| {
            layout.resolve(1400., height).groups.into_iter().find(|g| g.panels.contains(&panel)).unwrap().bounds
        };
        for platform in Platform::ALL {
            let mut proportional = WorkspacePreset::columns_layout(platform);
            insert_proof(&mut proportional);
            let mut layout = proportional.clone();
            fit_paint_columns(&mut layout);
            assert_eq!(layout.fit_height_groups, [10, 14]);
            for invalid in [vec![10, 10], vec![10, 999]] {
                let mut invalid_layout = layout.clone();
                invalid_layout.fit_height_groups = invalid;
                assert!(invalid_layout.validate().is_err());
            }
            // Exercise the original fit-height arrangement; its exact shipped
            // default now includes the separately tested palette group.
            for stack in &mut layout.column_stacks {
                if layout.collapsed.iter().any(|c| c.root == stack.column) {
                    stack.open_column = Some(stack.column);
                }
            }
            let column = |layout: &DockLayout, height| {
                bounds(layout, height, Panel::Brushes).height
                    + bounds(layout, height, Panel::ToolSettings).height
                    + bounds(layout, height, Panel::Color).height
                    + WORKSPACE_SPACING * 2.
            };
            assert!((bounds(&layout, 1000., Panel::Color).height - bounds(&proportional, 1000., Panel::Color).height).abs() < WORKSPACE_SPACING);
            layout.measurements = [(Panel::Color, 300.), (Panel::Navigator, 180.), (Panel::Brushes, 900.)]
                .map(|(panel, content_height)| PanelMeasurement { panel, tab_width: 0., content_height, scroll: None })
                .to_vec();
            for height in [640., 1000., 1600.] {
                assert_eq!(bounds(&layout, height, Panel::Color).height, 300. + TAB_BAR_HEIGHT);
                assert_eq!(bounds(&layout, height, Panel::Navigator).height, 180. + TAB_BAR_HEIGHT);
                assert_eq!(bounds(&layout, height, Panel::Brushes).height, bounds(&layout, height, Panel::ToolSettings).height);
                let properties = bounds(&layout, height, Panel::Properties).height;
                let layers = bounds(&layout, height, Panel::Layers).height;
                assert!((properties / (properties + layers) - 0.4).abs() < 0.01);
            }
            let short = 380.;
            let minimum = TAB_BAR_HEIGHT + TILE_SIZE;
            assert_eq!(bounds(&layout, short, Panel::Brushes).height, minimum);
            assert_eq!(bounds(&layout, short, Panel::ToolSettings).height, minimum);
            assert!(bounds(&layout, short, Panel::Color).height < 300. + TAB_BAR_HEIGHT);

            let mut resized = layout.clone();
            let color = bounds(&resized, 1000., Panel::Color);
            let divider = resized.resolve(1400., 1000.).dividers.into_iter().find(|d| d.id == 4).unwrap().bounds;
            resized.resize(4, [divider.x + 10., divider.y + divider.height * 0.5 - 100.], [1400., 1000.]).unwrap();
            assert_eq!(resized.fit_height_groups, [14]);
            let dragged = bounds(&resized, 1000., Panel::Color);
            assert!((dragged.height - color.height - 100.).abs() < 0.5);
            assert!((bounds(&resized, 1400., Panel::Color).height - dragged.height).abs() > 50.);

            let mut moved = layout.clone();
            moved.move_item([1400., 1000.], DockItem::Group { group: 10 }, DockTarget::Split { group: 6, edge: Edge::Top }).unwrap();
            assert_eq!(moved.fit_height_groups, [14, 10]);
            assert_eq!(bounds(&moved, 1000., Panel::Color).height, 300. + TAB_BAR_HEIGHT);
            assert_eq!(bounds(&moved, 1000., Panel::Color).y, bounds(&layout, 1000., Panel::Brushes).y);
            moved.move_item([1400., 1000.], DockItem::Group { group: 10 }, DockTarget::Tab { group: 7, index: None }).unwrap();
            assert_eq!(moved.fit_height_groups, [14]);
            assert!((column(&layout, 1000.) - column(&resized, 1000.)).abs() < 0.01);
        }
    }
}
