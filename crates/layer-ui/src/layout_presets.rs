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

    pub fn initial(platform: crate::Platform, mobile_browser: bool) -> Self {
        if matches!(platform, crate::Platform::Android | crate::Platform::Ios)
            || (platform == crate::Platform::Web && mobile_browser)
        { Self::Painter } else { Self::Illustrator }
    }

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
            (Panel::Adjustments, Panel::Layers),
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
        if self == Self::Photographer {
            if let Some(DockNode::Tabs {panels,active,..})=layout.node_mut(group) {
                *panels=vec![Panel::Histogram,Panel::Waveform];
                *active=Panel::Histogram;
            }
            fit_right_column(&mut layout);
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
                Command {command:C::Hand}, Command {command:C::Zoom}, Command {command:C::Eyedropper}, Color,
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
                    C::Fill=>S::Fill, C::Gradient=>S::Gradient, C::Move|C::EditPoints=>S::Operation, C::Figure=>S::Figure, C::Ruler=>S::Ruler,
                    _=>continue,
                };
                tile.control=ToolSlot {slot};
            }
        }
    }

    fn painter_layout(platform: crate::Platform) -> DockLayout {
        use crate::CommandId::*;
        use ToolbarControl::{
            BrushSizeSlider, BrushStrengthSlider, Color, ColorPicker, Divider,
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
            .add_toolbar(None, "Brush controls", &[BrushSizeSlider, BrushStrengthSlider])
            .expect("built-in brush controls");
        let config = layout.panels.iter_mut().find(|p| p.id == panel).unwrap();
        config.tile_style = TileStyle::Medium;
        let opacity = config
            .tiles()
            .iter()
            .find(|t| t.control == BrushStrengthSlider)
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
        secondary.extent = TILE_SIZE + WORKSPACE_SPACING;
        layout.bands[1].extent = 0.;
        layout.bands.insert(2, secondary);
        layout.collapsed = vec![CollapsedColumn {
            root: 4,
            expanded_width: 0.,
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
                expanded_width: 0.,
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
    fit_right_column(layout);
    layout.fit_height_groups = vec![10, 14];
}

fn fit_right_column(layout: &mut DockLayout) {
    let group = |id| Box::new(layout.node(id).cloned().expect("default primary panel"));
    let root = DockNode::Split {id:12,axis:Axis::Vertical,fraction:0.25,first:group(14),
        second:Box::new(DockNode::Split {id:13,axis:Axis::Vertical,fraction:0.4,first:group(15),second:group(16)})};
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
        assert!(Panel::BrushSets.default_width(1600.) < Panel::Tools.default_width(1600.));
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
                (15, vec![Panel::Properties]),
                (16, vec![Panel::Layers, Panel::Adjustments]),
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
                assert_eq!(layers, group(Panel::Adjustments));
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
            let (anchor,selected)=if scopes {
                for panel in [Panel::Color,Panel::Palettes] {assert!(layout.panel(panel).is_ok());assert!(layout.panel_group(panel).is_none());}
                (Panel::Histogram,Panel::Waveform)
            } else {(Panel::Color,Panel::Palettes)};
            for (panel, anchor) in [
                (selected, anchor),
                (Panel::Proof, Panel::Navigator),
                (Panel::Stats, Panel::Brushes),
                (Panel::Adjustments, Panel::Layers),
            ] {
                let group = layout.panel_group(anchor).unwrap();
                let panels = layout.group_panels(group).unwrap();
                let index = panels.iter().position(|p| *p == anchor).unwrap();
                assert_eq!(panels[index + 1], panel);
                assert_eq!(layout.active_panel(panel), Some(anchor));
            }
            let group = layout.panel_group(anchor).unwrap();
            layout.select_tab(group, selected).unwrap();
            for (width, preferred) in [(1280., 242.), (1599., 242.), (1600., 280.), (1920., 280.)] {
                let geometry = layout.resolved([width, 1000.]);
                for group in geometry.groups.iter().filter(|g| g.active.kind() == PanelKind::Content) {
                    assert_eq!(group.bounds.width, preferred, "{preset:?} {platform:?} {group:?} at {width}");
                }
            }
            let mut manual = layout.clone();
            manual.bands.iter_mut().find(|b| b.root.find(group).is_some()).unwrap().extent = 306.;
            for width in [1280., 1920.] {
                assert_eq!(manual.resolved([width, 1000.]).groups.iter().find(|g| g.id == group).unwrap().bounds.width, 300.);
            }
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
            for width in [1280., 1920.] {
                let content = first.default_width(width);
                layout.measurements[0].content_height = content;
                let fitted = layout.resolved([width, 1000.]).groups.into_iter().find(|g| g.id == group).unwrap().bounds;
                assert_eq!(fitted.height, content + TAB_BAR_HEIGHT);
                layout.select_tab(group, second).unwrap();
                assert_eq!(layout.resolved([width, 1000.]).groups.into_iter().find(|g| g.id == group).unwrap().bounds, fitted);
            }
            layout.measurements[0].content_height = 340.;
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
    fn paint_and_photo_share_primary_column_proportions() {
        for platform in Platform::ALL {
            let mut paint = WorkspacePreset::Illustrator.layout(platform);
            paint.open_default_columns(platform);
            let photo = WorkspacePreset::Photographer.layout(platform);
            assert!(photo.fit_height_groups.is_empty());
            for [width, height] in [[1200., 800.], [1600., 1000.], [1920., 1600.]] {
                let proportions = |layout: &DockLayout, panels: [Panel; 3]| {
                    let resolved = layout.resolve(width, height);
                    let [top, properties, layers] = panels.map(|panel| {
                        resolved.groups.iter().find(|g| g.panels.contains(&panel)).unwrap().bounds.height
                    });
                    [top / (top + properties + layers + WORKSPACE_SPACING), properties / (properties + layers)]
                };
                for layout_proportions in [
                    proportions(&paint, [Panel::Navigator, Panel::Properties, Panel::Layers]),
                    proportions(&photo, [Panel::Histogram, Panel::Properties, Panel::Layers]),
                ] {
                    for (actual, expected) in layout_proportions.into_iter().zip([0.25, 0.4]) {
                        assert!((actual - expected).abs() < 0.001, "{platform:?} {width}x{height}: {layout_proportions:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn photo_column_proportions_preserve_visible_rows() {
        for platform in Platform::ALL {
            let mut layout=WorkspacePreset::Photographer.layout(platform);layout.open_default_columns(platform);
            layout.measurements=[
                PanelMeasurement{panel:Panel::Histogram,content_height:260.,tab_width:111.,scroll:None},
                PanelMeasurement{panel:Panel::Waveform,content_height:260.,tab_width:110.,scroll:None},
                PanelMeasurement{panel:Panel::Properties,content_height:160.,tab_width:111.,scroll:Some(PanelScrollMeasurement{fixed_height:0.,unit_height:0.})},
                PanelMeasurement{panel:Panel::Adjustments,content_height:3720.,tab_width:111.,scroll:Some(PanelScrollMeasurement{fixed_height:40.,unit_height:64.})},
                PanelMeasurement{panel:Panel::Layers,content_height:172.,tab_width:85.,scroll:Some(PanelScrollMeasurement{fixed_height:92.,unit_height:40.})},
            ].to_vec();
            for width in [744.,960.,1200.] {
                for height in [600.,660.,800.,1000.,1600.] {
                    let resolved=layout.workspace(width,height,crate::HEADER_HEIGHT,0.);
                    let bounds=|panel|resolved.groups.iter().find(|g|g.panels.contains(&panel)).unwrap().bounds;
                    let scopes=bounds(Panel::Histogram);let properties=bounds(Panel::Properties);let layers=bounds(Panel::Layers);
                    let row_minimum = if height >= 800. { 132. } else { TILE_SIZE };
                    assert!(layers.height-TAB_BAR_HEIGHT>=row_minimum,"{platform:?} {width}x{height}: {layers:?}");
                    assert!(properties.height-TAB_BAR_HEIGHT>=TILE_SIZE,"{platform:?} {width}x{height}: {properties:?}");
                    assert_eq!(scopes,bounds(Panel::Waveform));
                    assert!(scopes.y+scopes.height<properties.y);assert!(properties.y+properties.height<layers.y);assert_eq!(properties.x,layers.x);
                    assert!(layers.y+layers.height<=height);
                    if height>=800. {assert!(scopes.height>=260.+TAB_BAR_HEIGHT);}
                    if height==1000. {
                        assert!((properties.height/(properties.height+layers.height)-0.4).abs()<0.01,"{platform:?} {width}x{height}: {properties:?} {layers:?}");
                        assert!(properties.height-TAB_BAR_HEIGHT>=160.);
                    }
                }
            }
        }
    }

    fn measured_panel_layout(preset: WorkspacePreset, platform: Platform) -> DockLayout {
        let mut layout = preset.layout(platform);
        layout.open_default_columns(platform);
        layout.header_presentation.height = 48.;
        let scroll = |fixed_height, unit_height| Some(PanelScrollMeasurement { fixed_height, unit_height });
        layout.measurements = [
            (Panel::Navigator, 213.5, scroll(44., 0.)),
            (Panel::Proof, 16., None),
            (Panel::Properties, 157., scroll(0., 0.)),
            (Panel::Adjustments, 3760., scroll(40., 64.)),
            (Panel::Layers, 172., scroll(92., 40.)),
            (Panel::Histogram, 260., None),
            (Panel::Waveform, 260., None),
            (Panel::Color, 300., None),
            (Panel::Palettes, 500., scroll(60., 38.)),
            (Panel::Brushes, 900., scroll(0., 0.)),
        ].map(|(panel, content_height, scroll)| PanelMeasurement { panel, content_height, scroll, tab_width: 0. }).to_vec();
        layout
    }

    #[test]
    fn continuous_viewports_keep_their_minimum_before_sibling_preferred_rows() {
        for platform in Platform::ALL {
            let mut layout = measured_panel_layout(WorkspacePreset::Illustrator, platform);
            let navigator_group = layout.panel_group(Panel::Navigator).unwrap();
            let layers_group = layout.panel_group(Panel::Layers).unwrap();
            for rows in [0., 0.175, 1., 2., 200.] {
                layout.measurements.iter_mut().find(|m| m.panel == Panel::Layers).unwrap().content_height = 92. + rows * 40.;
                for properties_height in [7., 157., 3000.] {
                    layout.measurements.iter_mut().find(|m| m.panel == Panel::Properties).unwrap().content_height = properties_height;
                    for height in [600., 686., 691., 695., 720., 722., 1000.] {
                        let durable = crate::durable_layout(&layout);
                        let resolved = layout.workspace(1257., height, 48., 0.);
                        let group = |panel| resolved.groups.iter().find(|g| g.panels.contains(&panel)).unwrap();
                        for panel in [Panel::Navigator, Panel::Properties, Panel::Layers] {
                            let g = group(panel);
                            assert!(g.body().height + 0.001 >= g.min_body_height, "{platform:?} height={height} rows={rows} {panel:?}: {:?}", g.bounds);
                        }
                        assert_eq!(group(Panel::Navigator).min_body_height, 44. + TILE_SIZE);
                        for (id, panel) in [(navigator_group, Panel::Proof), (layers_group, Panel::Adjustments)] {
                            layout.select_tab(id, panel).unwrap();
                            let switched = layout.workspace(1257., height, 48., 0.);
                            for g in &resolved.groups {
                                assert_eq!(switched.groups.iter().find(|next| next.id == g.id).unwrap().bounds, g.bounds);
                            }
                        }
                        layout.select_tab(navigator_group, Panel::Navigator).unwrap();
                        layout.select_tab(layers_group, Panel::Layers).unwrap();
                        assert_eq!(crate::durable_layout(&layout), durable);
                    }
                }
            }
        }
    }

    #[test]
    fn constrained_columns_expose_scrollable_bodies_without_editing_saved_layouts() {
        for platform in Platform::ALL {
            for preset in [WorkspacePreset::Illustrator, WorkspacePreset::Photographer] {
                let mut layout = measured_panel_layout(preset, platform);
                for fitted in [true, false] {
                    if !fitted { layout.fit_height_groups.clear(); }
                    let durable = crate::durable_layout(&layout);
                    for (height, clearance) in [(360., 0.), (820., 422.), (900., 502.)] {
                        layout.bottom_inset = clearance;
                        let resolved = layout.workspace(640., height, 48., 0.);
                        let panels = [if preset == WorkspacePreset::Illustrator { Panel::Navigator } else { Panel::Histogram }, Panel::Properties, Panel::Layers];
                        let groups: Vec<_> = panels.iter().map(|p| resolved.groups.iter().find(|g| g.panels.contains(p)).unwrap()).collect();
                        for g in &groups {
                            assert!(g.body().height + 0.001 >= g.min_body_height.min(TILE_SIZE), "{platform:?} {preset:?} {height}/{clearance}: {:?}", g.bounds);
                            assert!(g.bounds.y + g.bounds.height <= height - clearance - WORKSPACE_SPACING + 0.001);
                        }
                        for pair in groups.windows(2) { assert!(pair[0].bounds.y + pair[0].bounds.height <= pair[1].bounds.y); }
                        assert_eq!(groups[0].min_body_height, if preset == WorkspacePreset::Illustrator { 80. } else { 260. });
                        assert_eq!(groups[1].min_body_height, 36.);
                        assert_eq!(groups[2].min_body_height, 132.);
                        assert_eq!(crate::durable_layout(&layout), durable);
                    }
                    layout.bottom_inset = 0.;
                    for height in [80., 128., 240.] {
                        for g in layout.workspace(640., height, 48., 0.).groups {
                            assert!(g.bounds.height.is_finite() && g.bounds.height >= 0.);
                            assert!(g.bounds.y + g.bounds.height <= height - WORKSPACE_SPACING + 0.001);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fixed_panel_content_stays_whole_when_neighboring_scroll_targets_do_not_fit() {
        for fitted in [false, true] {
            let mut layout = WorkspacePreset::Illustrator.layout(Platform::Ios);
            layout.open_default_columns(Platform::Ios);
            let first = Box::new(layout.node(14).unwrap().clone());
            let second = Box::new(layout.node(16).unwrap().clone());
            layout.bands.iter_mut().find(|b| b.id == 11).unwrap().root = DockNode::Split {
                id: 12, axis: Axis::Vertical, fraction: 0.4, first, second,
            };
            layout.fit_height_groups = if fitted { vec![14] } else { vec![] };
            layout.measurements = [
                PanelMeasurement { panel: Panel::Navigator, tab_width: 100., content_height: 100., scroll: None },
                PanelMeasurement { panel: Panel::Layers, tab_width: 86., content_height: 200.,
                    scroll: Some(PanelScrollMeasurement { fixed_height: 0., unit_height: 40. }) },
            ].to_vec();
            let resolved = layout.resolve(1180., 256.);
            let bounds = |panel| resolved.groups.iter().find(|g| g.panels.contains(&panel)).unwrap().bounds;
            assert_eq!(bounds(Panel::Navigator).height, 100. + TAB_BAR_HEIGHT);
            assert_eq!(bounds(Panel::Layers).height, 256. - WORKSPACE_SPACING - 100. - TAB_BAR_HEIGHT);
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
                .map(|(panel, content_height)| PanelMeasurement { panel, tab_width: 0., content_height,
                    scroll: (panel == Panel::Brushes).then_some(PanelScrollMeasurement { fixed_height: 0., unit_height: 0. }) })
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
            let brushes = bounds(&layout, short, Panel::Brushes);
            let tool = bounds(&layout, short, Panel::ToolSettings);
            let color = bounds(&layout, short, Panel::Color);
            assert_eq!(brushes.height, TAB_BAR_HEIGHT + TILE_SIZE);
            assert_eq!(tool.height, TAB_BAR_HEIGHT + TILE_SIZE);
            assert_eq!(color.height, short - brushes.height - tool.height - WORKSPACE_SPACING * 2.);
            assert!(brushes.y + brushes.height <= tool.y);
            assert!(tool.y + tool.height <= color.y);
            assert!(color.y + color.height <= short);
            assert!(color.height < 300. + TAB_BAR_HEIGHT);

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
