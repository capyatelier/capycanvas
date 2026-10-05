use super::*;
use std::collections::BTreeMap;

variants! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ToolSlotId {
        Drawing, Marquee, Lasso, AutomaticSelection, ManualSelection, Healing,
        PhotoFill, Fill, LassoFill, Blend, Operation, Figure, Ruler, Gradient,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolVariant {
    Command { command: CommandId },
    BrushGroup { group: ToolGroup },
    BrushPreset { id: u32 },
    Figure { shape: FigureShape },
    Ruler { kind: RulerKind },
    Gradient {shape:layer_core::GradientShape},
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolControlGroup {
    Slot(ToolSlotId),
    Brush(Tool),
    Drawing,
    Sculpt,
    Selection,
}
impl ToolbarControl {
    pub(crate) fn tool_group(self) -> Option<ToolControlGroup> {
        use ToolControlGroup as G;
        Some(match self {
            Self::ToolSlot { slot } => G::Slot(slot),
            Self::Command { command } => match command {
                CommandId::DrawingBrush => G::Drawing,
                CommandId::Sculpt => G::Sculpt,
                CommandId::Select => G::Selection,
                CommandId::Figure => G::Slot(ToolSlotId::Figure),
                CommandId::Ruler => G::Slot(ToolSlotId::Ruler),
                CommandId::Gradient => G::Slot(ToolSlotId::Gradient),
                CommandId::Move => G::Slot(ToolSlotId::Operation),
                CommandId::Fill => G::Slot(ToolSlotId::Fill),
                CommandId::LassoFill => G::Slot(ToolSlotId::LassoFill),
                _ => G::Brush(command.paint_tool()?),
            },
            _ => return None,
        })
    }
    pub fn has_variants(self) -> bool { self.tool_group().is_some() }
}
impl ToolControlGroup {
    fn variants(self) -> Vec<ToolVariant> {
        match self {
            Self::Slot(slot) => slot.variants().to_vec(),
            Self::Selection => SelectionTool::ALL.into_iter().map(|t| command(t.command())).collect(),
            Self::Brush(tool) => {
                let groups: Vec<_> = ToolGroup::ALL.into_iter().filter(|g| g.tool() == tool).collect();
                if groups.len() > 1 {
                    groups.into_iter().map(|group| ToolVariant::BrushGroup { group }).collect()
                } else {
                    tools::brush_ids().filter(|&id| tools::group(id).tool() == tool).map(|id| ToolVariant::BrushPreset { id }).collect()
                }
            },
            Self::Drawing | Self::Sculpt => ToolGroup::ALL.into_iter()
                .filter(|g| if self == Self::Drawing { tools::is_drawing(g.tool()) } else { tools::is_sculpt(g.tool()) })
                .map(|group| ToolVariant::BrushGroup { group }).collect(),
        }
    }
    fn contains(self, variant: ToolVariant) -> bool {
        match self {
            Self::Slot(slot) => slot.variants().contains(&variant),
            Self::Selection => matches!(variant, ToolVariant::Command { command } if SelectionTool::ALL.iter().any(|t| t.command() == command)),
            Self::Brush(tool) => match variant {
                ToolVariant::BrushGroup { group } => group.tool() == tool && ToolGroup::ALL.iter().filter(|g| g.tool() == tool).count() > 1,
                ToolVariant::BrushPreset { id } => preset(id).is_ok() && tools::group(id).tool() == tool && ToolGroup::ALL.iter().filter(|g| g.tool() == tool).count() == 1,
                _ => false,
            },
            Self::Drawing => matches!(variant, ToolVariant::BrushGroup { group } if tools::is_drawing(group.tool())),
            Self::Sculpt => matches!(variant, ToolVariant::BrushGroup { group } if tools::is_sculpt(group.tool())),
        }
    }
    fn active(self, state: &UiState) -> bool {
        match self {
            Self::Slot(slot) => ToolVariant::active(state).is_some_and(|v| slot.variants().contains(&v)),
            Self::Selection => state.layer_tools.tool.selection_tool().is_some(),
            Self::Brush(tool) => state.layer_tools.tool == LayerCanvasTool::Paint && state.brush.tool == tool,
            Self::Drawing => state.layer_tools.tool == LayerCanvasTool::Paint && tools::is_drawing(state.brush.tool),
            Self::Sculpt => state.layer_tools.tool == LayerCanvasTool::Paint && tools::is_sculpt(state.brush.tool),
        }
    }
}
const fn command(command: CommandId) -> ToolVariant {
    ToolVariant::Command { command }
}
impl ToolSlotId {
    pub fn variants(self) -> &'static [ToolVariant] {
        use CommandId as C;
        use ToolVariant as V;
        match self {
            Self::Drawing => {
                const {
                    &[
                        command(C::Brush),
                        command(C::Pen),
                        command(C::Pencil),
                        command(C::Airbrush),
                        command(C::Decoration),
                    ]
                }
            }
            Self::Marquee => const { &[command(C::RectangleSelect), command(C::EllipseSelect)] },
            Self::Lasso => const { &[command(C::Lasso), command(C::PolygonSelect)] },
            Self::AutomaticSelection => {
                const { &[command(C::AutoSelect), command(C::ColorSelect)] }
            }
            Self::ManualSelection => {
                const {
                    &[
                        command(C::Lasso),
                        command(C::RectangleSelect),
                        command(C::EllipseSelect),
                        command(C::PolygonSelect),
                        command(C::SelectionBrush),
                    ]
                }
            }
            Self::Healing => const { &[command(C::SpotHeal), command(C::Heal)] },
            Self::PhotoFill => {
                const {
                    &[
                        V::Gradient {shape:layer_core::GradientShape::Linear},
                        V::Gradient {shape:layer_core::GradientShape::Radial},
                        V::Gradient {shape:layer_core::GradientShape::Reflected},
                        command(C::Fill),
                        command(C::LassoFill),
                        command(C::EncloseFill),
                    ]
                }
            }
            Self::Fill => const { &[command(C::Fill), command(C::LassoFill), command(C::EncloseFill)] },
            Self::LassoFill => const { &[command(C::LassoFill), command(C::EncloseFill)] },
            Self::Blend => const { &[command(C::Blend), command(C::Clone)] },
            Self::Operation => const { &[command(C::Move), command(C::ScaleRotate)] },
            Self::Figure => {
                const {
                    &[
                        V::Figure {
                            shape: FigureShape::Line,
                        },
                        V::Figure {
                            shape: FigureShape::Rectangle,
                        },
                        V::Figure {
                            shape: FigureShape::Ellipse,
                        },
                    ]
                }
            }
            Self::Ruler => {
                const {
                    &[
                        V::Ruler {
                            kind: RulerKind::Straight,
                        },
                        V::Ruler {
                            kind: RulerKind::Parallel,
                        },
                        V::Ruler {
                            kind: RulerKind::Radial,
                        },
                    ]
                }
            }
            Self::Gradient => {
                const {
                    &[
                        V::Gradient {shape:layer_core::GradientShape::Linear},
                        V::Gradient {shape:layer_core::GradientShape::Radial},
                        V::Gradient {shape:layer_core::GradientShape::Reflected},
                    ]
                }
            }
        }
    }
    pub(crate) fn label(self, localization: &Localizer) -> String {
        match self {
            Self::Drawing => CommandId::DrawingBrush
                .localized_label(localization)
                .to_string(),
            Self::ManualSelection => CommandId::Select.localized_label(localization).to_string(),
            Self::Figure => CommandId::Figure.localized_label(localization).to_string(),
            Self::Ruler => CommandId::Ruler.localized_label(localization).to_string(),
            Self::LassoFill => CommandId::LassoFill.localized_label(localization).to_string(),
            Self::Gradient => CommandId::Gradient
                .localized_label(localization)
                .to_string(),
            _ => {
                let mut commands = Vec::new();
                for variant in self.variants() {
                    let id = variant.command();
                    if !commands.contains(&id) {
                        commands.push(id);
                    }
                }
                commands
                    .iter()
                    .map(|c| c.localized_label(localization).to_string())
                    .collect::<Vec<_>>()
                    .join(" / ")
            }
        }
    }
}
impl ToolVariant {
    pub fn command(self) -> CommandId {
        match self {
            Self::Command { command } => command,
            Self::BrushGroup { group } => group.tool().command(),
            Self::BrushPreset { id } => preset(id).map(|_| tools::group(id).tool().command()).unwrap_or(CommandId::Brush),
            Self::Figure { .. } => CommandId::Figure,
            Self::Ruler { .. } => CommandId::Ruler,
            Self::Gradient { .. } => CommandId::Gradient,
        }
    }
    pub(crate) fn control(self) -> ToolbarControl {
        if let Self::BrushPreset { id } = self { return ToolbarControl::Brush { id }; }
        ToolbarControl::Command {
            command: self.command(),
        }
    }
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Command { command } => command.icon().unwrap_or("menu"),
            Self::BrushGroup { group } => group.icon(),
            Self::BrushPreset { id } => preset(id).map(|_| tools::group(id).icon()).unwrap_or("brush"),
            Self::Figure { shape } => match shape {
                FigureShape::Line => "line",
                FigureShape::Rectangle => "rectangle",
                FigureShape::Ellipse => "ellipse",
            },
            Self::Ruler { kind } => match kind {
                RulerKind::Straight => "ruler",
                RulerKind::Parallel => "ruler-parallel",
                RulerKind::Radial => "ruler-radial",
            },
            Self::Gradient {shape}=>match shape {layer_core::GradientShape::Linear=>"gradient",layer_core::GradientShape::Radial=>"gradient-radial",layer_core::GradientShape::Reflected=>"gradient-reflected"},
        }
    }
    pub(crate) fn icon_in(self, state: &UiState) -> &'static str {
        if let Self::Command { command } = self {
            state.commands.iter().find(|c| c.id == command).and_then(|c| c.icon).unwrap_or(self.icon())
        } else { self.icon() }
    }
    fn label(self, localization: &Localizer) -> String {
        let id = match self {
            Self::Command { command } => return command.localized_label(localization).to_string(),
            Self::BrushGroup { group } => return group.localized_label(localization).to_string(),
            Self::BrushPreset { id } => return tools::brush_label_localized(id, localization).map(|s| s.to_string()).unwrap_or_default(),
            Self::Figure {
                shape: FigureShape::Line,
            } => MessageId::TOOL_FIGURES_LINE,
            Self::Figure {
                shape: FigureShape::Rectangle,
            } => MessageId::TOOL_FIGURES_RECTANGLE,
            Self::Figure {
                shape: FigureShape::Ellipse,
            } => MessageId::TOOL_FIGURES_ELLIPSE,
            Self::Ruler {
                kind: RulerKind::Straight,
            } => MessageId::TOOL_RULERS_STRAIGHT,
            Self::Ruler {
                kind: RulerKind::Parallel,
            } => MessageId::TOOL_RULERS_PARALLEL,
            Self::Ruler {
                kind: RulerKind::Radial,
            } => MessageId::TOOL_RULERS_RADIAL,
            Self::Gradient {shape}=>match shape {layer_core::GradientShape::Linear=>MessageId::RESOURCES_CHOICE_GRADIENT_FILL_STYLE_LINEAR,
                layer_core::GradientShape::Radial=>MessageId::RESOURCES_CHOICE_GRADIENT_FILL_STYLE_RADIAL,
                layer_core::GradientShape::Reflected=>MessageId::RESOURCES_CHOICE_GRADIENT_FILL_STYLE_REFLECTED},
        };
        localization.text(id).to_string()
    }
    pub(crate) fn active(state: &UiState) -> Option<Self> {
        let tool = state.layer_tools.tool;
        Some(match tool {
            LayerCanvasTool::Figure { shape, .. } => Self::Figure { shape },
            LayerCanvasTool::Ruler { kind } => Self::Ruler { kind },
            LayerCanvasTool::Gradient {shape}=>Self::Gradient {shape},
            LayerCanvasTool::Paint => command(state.brush.tool.command()),
            _ => command(if let Some(selection) = tool.selection_tool() {
                selection.command()
            } else {
                match tool {
                    LayerCanvasTool::Move => CommandId::Move,
                    LayerCanvasTool::Transform => CommandId::ScaleRotate,
                    LayerCanvasTool::Region { fill: true, .. } => CommandId::Fill,
                    LayerCanvasTool::LassoFill => CommandId::LassoFill,
                    LayerCanvasTool::EncloseFill { .. } => CommandId::EncloseFill,
                    LayerCanvasTool::Crop => CommandId::Crop,
                    LayerCanvasTool::Hand => CommandId::Hand,
                    _ => return None,
                }
            }),
        })
    }
    fn is_active(self, state: &UiState) -> bool {
        match self {
            Self::BrushGroup { group } => state.layer_tools.tool == LayerCanvasTool::Paint && tools::group(state.brush.preset) == group,
            Self::BrushPreset { id } => state.layer_tools.tool == LayerCanvasTool::Paint && state.brush.preset == id,
            _ => Self::active(state) == Some(self),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSlotMemory {
    pub choices: BTreeMap<ToolSlotId, ToolVariant>,
}
impl ToolSlotMemory {
    fn get(&self, slot: ToolSlotId) -> Option<ToolVariant> { self.choices.get(&slot).copied() }
    fn remember(&mut self, slot: ToolSlotId, variant: ToolVariant) { self.choices.insert(slot, variant); }
}
impl DockLayout {
    pub(crate) fn tool_slots(&self) -> impl Iterator<Item = (DrawerAnchor, ToolSlotId)> + '_ {
        self.panels
            .iter()
            .flat_map(|p| {
                p.tiles().iter().map(move |t| {
                    (
                        DrawerAnchor::Tile {
                            panel: p.id,
                            tile: t.id,
                        },
                        t.control,
                    )
                })
            })
            .chain(self.header.entries().filter_map(|e| match e.item {
                HeaderItem::Tool { control } => Some((DrawerAnchor::Header { id: e.id }, control)),
                _ => None,
            }))
            .filter_map(|(anchor, control)| {
                if let Some(ToolControlGroup::Slot(slot)) = control.tool_group() {
                    Some((anchor, slot))
                } else {
                    None
                }
            })
    }
    pub(crate) fn anchor_control(&self, anchor: DrawerAnchor) -> Option<ToolbarControl> {
        match anchor {
            DrawerAnchor::Tile { panel, tile } => self
                .panel(panel)
                .ok()?
                .tiles()
                .iter()
                .find(|t| t.id == tile)
                .map(|t| t.control),
            DrawerAnchor::Header { id } => match self.header.entry(id).ok()?.item {
                HeaderItem::Tool { control } => Some(control),
                _ => None,
            },
            _ => None,
        }
    }
}
impl UiState {
    pub(crate) fn slot_variant(&self, slot: ToolSlotId) -> ToolVariant {
        ToolVariant::active(self)
            .filter(|v| slot.variants().contains(v))
            .or_else(|| self.tool_slots.get(slot))
            .unwrap_or(slot.variants()[0])
    }
    pub(crate) fn resolve_slot(
        &self,
        slot: ToolSlotId,
    ) -> (ToolChoice, bool, String, ToolbarControl) {
        let variant = self.slot_variant(slot);
        let mut choice = tool_choice_localized(variant.control(), &self.localization);
        choice.control = ToolbarControl::ToolSlot { slot };
        choice.label = variant.label(&self.localization);
        choice.icon = variant.icon_in(self);
        choice.selected = ToolVariant::active(self) == Some(variant);
        let enabled = tool_state(self, variant.control()).0;
        let shortcut = self
            .commands
            .iter()
            .find(|c| c.id == variant.command())
            .map(|c| c.shortcut.as_str())
            .unwrap_or("");
        let title = format!("{} · {}", choice.label, slot.label(&self.localization));
        let tooltip = if shortcut.is_empty() {
            title
        } else {
            format!("{title} ({shortcut})")
        };
        (choice, enabled, tooltip, variant.control())
    }
    pub(crate) fn resolve_group(&self, control: ToolbarControl) -> Option<(ToolChoice, bool, String, ToolbarControl)> {
        let ToolControlGroup::Slot(slot) = control.tool_group()? else { return None; };
        let (mut choice, enabled, tooltip, resolved) = self.resolve_slot(slot);
        choice.control = control;
        Some((choice, enabled, tooltip, resolved))
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn group_choices(&self, group: ToolControlGroup, anchor: Option<DrawerAnchor>) -> Vec<(ToolVariant, ToolSetItem)> {
        group.variants().into_iter().filter(|v| v.command().available_on(self.state.platform))
            .map(|variant| (variant, ToolSetItem {
                enabled: self.command(variant.command()).enabled,
                label: variant.label(self.localization()).into(),
                icon: variant.icon_in(&self.state),
                action: anchor.map_or_else(|| self.variant_action(variant), |anchor| UiAction::ChooseToolVariant { anchor, variant }),
                selected: variant.is_active(&self.state),
                preview: if let ToolVariant::BrushPreset { id } = variant { Some(id) } else { None },
            })).collect()
    }

    fn active_tool_group(&self) -> Option<(ToolControlGroup, Option<DrawerAnchor>)> {
        let layout = &self.state.workspace.layout;
        let origin = self.tool_origin.and_then(|(anchor, group)| {
            let anchor = if let DrawerAnchor::Tile { tile, .. } = anchor {
                layout.panels.iter().find_map(|panel| panel.tiles().iter().any(|t| t.id == tile)
                    .then_some(DrawerAnchor::Tile { panel: panel.id, tile }))?
            } else { anchor };
            Some((anchor, group))
        });
        if let Some((anchor, group)) = origin
            && layout.anchor_control(anchor).and_then(|c| c.tool_group()) == Some(group)
            && group.active(&self.state) {
            return Some((group, Some(anchor)));
        }
        if let Some((anchor, slot)) = layout.tool_slots().find(|(_, slot)| ToolControlGroup::Slot(*slot).active(&self.state)) {
            return Some((ToolControlGroup::Slot(slot), Some(anchor)));
        }
        let group = if self.layer_interaction.tool.selection_tool().is_some() {
            ToolControlGroup::Selection
        } else if matches!(self.layer_interaction.tool, LayerCanvasTool::LassoFill | LayerCanvasTool::EncloseFill { .. }) {
            ToolControlGroup::Slot(ToolSlotId::LassoFill)
        } else if self.layer_interaction.tool == LayerCanvasTool::Transform {
            ToolControlGroup::Slot(ToolSlotId::Operation)
        } else {
            ToolVariant::active(&self.state)?.control().tool_group()?
        };
        Some((group, None))
    }

    fn group_tool_set(&self, group: ToolControlGroup, anchor: Option<DrawerAnchor>) -> ToolSetView {
        let mut view = tools::view(&self.state.brush, self.layer_interaction.tool, self.localization());
        if matches!(group, ToolControlGroup::Drawing | ToolControlGroup::Sculpt) { return view; }
        let lasso = ToolControlGroup::Slot(ToolSlotId::LassoFill);
        let remembered_lasso = self.group_variant(lasso);
        let choices = self.group_choices(group, anchor).into_iter().filter_map(|(variant, mut item)| {
            if lasso.contains(variant) {
                if variant != remembered_lasso { return None; }
                item.label = CommandId::LassoFill.localized_label(self.localization());
                item.icon = CommandId::LassoFill.icon().unwrap();
                item.selected = lasso.active(&self.state);
            }
            Some(item)
        }).collect();
        if lasso.active(&self.state) {
            view.subtools = self.group_choices(lasso, anchor).into_iter().map(|(_, item)| item).collect();
        }
        match group {
            ToolControlGroup::Slot(_) => {
                if self.layer_interaction.tool == LayerCanvasTool::Paint {
                    view.subtools.splice(0..0, std::mem::take(&mut view.groups));
                } else if self.layer_interaction.tool.selection_tool().is_some() {
                    view.subtools.clear();
                }
                view.groups = choices;
            },
            ToolControlGroup::Selection => { view.groups.clear(); view.subtools = choices; },
            ToolControlGroup::Brush(tool) => {
                if ToolGroup::ALL.iter().filter(|g| g.tool() == tool).count() > 1 { view.groups = choices; }
                else { view.subtools = choices; }
            },
            ToolControlGroup::Drawing | ToolControlGroup::Sculpt => (),
        }
        view
    }

    pub(super) fn current_tool_set(&self) -> ToolSetView {
        let Some((group, anchor)) = self.active_tool_group() else {
            return tools::view(&self.state.brush, self.layer_interaction.tool, self.localization());
        };
        let mut view = self.group_tool_set(group, anchor);
        if matches!(group, ToolControlGroup::Slot(ToolSlotId::Gradient))
            || self.layer_interaction.tool.selection_tool().is_some() && matches!(group, ToolControlGroup::Slot(_)) {
            view.subtools = std::mem::take(&mut view.groups);
        }
        view
    }

    pub(crate) fn variant_action(&self, variant: ToolVariant) -> UiAction {
        match variant {
            ToolVariant::Command { command } => UiAction::Invoke { command },
            ToolVariant::BrushGroup { group } => UiAction::SelectBrushSet { group },
            ToolVariant::BrushPreset { id } => UiAction::SelectBrush { id },
            ToolVariant::Figure { shape } => UiAction::Layer {
                action: LayerAction::Tool {
                    tool: LayerCanvasTool::Figure {
                        shape,
                        paint: if shape == FigureShape::Line {
                            FigurePaint::Outline
                        } else {
                            self.layer_interaction.figure.1
                        },
                    },
                },
            },
            ToolVariant::Ruler { kind } => UiAction::Layer {
                action: LayerAction::Tool {
                    tool: LayerCanvasTool::Ruler { kind },
                },
            },
            ToolVariant::Gradient {shape}=>UiAction::Layer {action:LayerAction::Tool {tool:LayerCanvasTool::Gradient {shape}}},
        }
    }
    pub(crate) fn tool_variants_menu(&self, anchor: DrawerAnchor) -> Result<ContextMenu, String> {
        let Some(control) = self.state.workspace.layout.anchor_control(anchor)
        else {
            return Err(self
                .localization()
                .text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)
                .to_string());
        };
        let Some(group) = control.tool_group() else { return Err(self.localization().text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET).to_string()); };
        let remembered = self.group_variant(group);
        let items = self.group_choices(group, Some(anchor)).into_iter()
            .map(|(variant, choice)| {
                let mut item = ContextMenuItem::command(
                    choice.label.to_string(), choice.action,
                );
                item.icon = Some(choice.icon);
                item.selected = Some(remembered == variant);
                item.enabled = choice.enabled;
                if let Some(reason) = self.command_disabled_reason(variant.command()) {
                    item.hint = reason;
                }
                item
            })
            .collect();
        Ok(ContextMenu {
            title: match group { ToolControlGroup::Slot(slot) => slot.label(self.localization()), _ => tool_choice_localized(control, self.localization()).label },
            sections: vec![items],
        })
    }
    fn group_variant(&self, group: ToolControlGroup) -> ToolVariant {
        let id = match group {
            ToolControlGroup::Slot(slot) => return self.state.slot_variant(slot),
            ToolControlGroup::Selection => return command(self.selection_tools.options.tool.command()),
            ToolControlGroup::Brush(tool) => self.tools.command_preset_in(tool.command(), &self.state.brush, self.layer_interaction.tool).unwrap(),
            ToolControlGroup::Drawing => self.tools.command_preset_in(CommandId::DrawingBrush, &self.state.brush, self.layer_interaction.tool).unwrap(),
            ToolControlGroup::Sculpt => self.tools.command_preset_in(CommandId::Sculpt, &self.state.brush, self.layer_interaction.tool).unwrap(),
        };
        let variant = ToolVariant::BrushGroup { group: tools::group(id) };
        if group.contains(variant) { variant } else { ToolVariant::BrushPreset { id } }
    }
    pub(crate) fn remember_tool_slots(&mut self) {
        if self.temporary_tool()
            || self.layer_interaction.tool.picks_color()
        {
            return;
        }
        let Some(variant) = ToolVariant::active(&self.state) else {
            return;
        };
        for slot in ToolSlotId::ALL {
            if slot.variants().contains(&variant) {
                self.state.tool_slots.remember(slot, variant);
            }
        }
    }
    pub(crate) fn update_slot_drawer(&mut self) -> bool {
        let Some(drawer) = self.state.customization.drawer.as_ref() else {
            return false;
        };
        let anchor = drawer.anchor;
        let control = self.state.workspace.layout.anchor_control(anchor);
        let active = ToolVariant::active(&self.state);
        let slot = match control {
            Some(ToolbarControl::ToolOptions { .. }) => self.active_tool_group().and_then(|(group, origin)| {
                if let ToolControlGroup::Slot(slot) = group { origin.map(|origin| (origin, slot)) } else { None }
            }),
            Some(control) => match control.tool_group() {
                Some(ToolControlGroup::Slot(slot)) => Some((anchor, slot)),
                Some(group) => {
                    if !group.active(&self.state) {
                        if !self.interaction.applying_hold && self.interaction.hold_base.is_none() {
                            self.state.customization.drawer = None;
                            return true;
                        }
                        return false;
                    }
                    let columns = control.drawer_columns().unwrap();
                    let drawer = self.state.customization.drawer.as_mut().unwrap();
                    if drawer.columns != columns || drawer.tool_set.is_some() {
                        drawer.columns = columns;
                        drawer.tool_set = None;
                        return true;
                    }
                    return false;
                },
                _ => return false,
            },
            _ => return false,
        };
        let (columns, tool_set) = if let Some((origin, slot)) = slot {
            if active.is_none_or(|v| !slot.variants().contains(&v)) {
                if !self.interaction.applying_hold && self.interaction.hold_base.is_none() {
                    self.state.customization.drawer = None;
                    return true;
                }
                return false;
            }
            (
                vec![vec![Panel::Brushes], vec![Panel::ToolSettings]],
                Some(self.group_tool_set(ToolControlGroup::Slot(slot), Some(origin))),
            )
        } else {
            let control = if self.state.layer_tools.tool.picks_color() {
                ToolbarControl::ColorPicker
            } else if let Some(variant) = active {
                variant.control()
            } else {
                return false;
            };
            let Some(columns) = control.drawer_columns() else {
                return false;
            };
            (columns, None)
        };
        let drawer = self.state.customization.drawer.as_mut().unwrap();
        if drawer.columns != columns || drawer.tool_set != tool_set {
            drawer.columns = columns;
            drawer.tool_set = tool_set;
            true
        } else {
            false
        }
    }
    pub(crate) fn slot_drawer_matches(&self) -> bool {
        self.state.customization.drawer.as_ref().is_some_and(|d| {
            match self.state.workspace.layout.anchor_control(d.anchor) {
                Some(ToolbarControl::ToolOptions { .. }) => true,
                Some(control) => control.tool_group().is_some_and(|g| g.active(&self.state)),
                _ => false,
            }
        })
    }
    fn show_tool_slot_drawer(&mut self, previous: DrawerAnchor, anchor: DrawerAnchor) {
        let Some(control) = self.state.workspace.layout.anchor_control(anchor) else { return; };
        let Some(columns) = control.drawer_columns() else { return; };
        let anchor = if matches!(
            self.state.workspace.layout.anchor_control(previous),
            Some(ToolbarControl::ToolOptions { .. })
        ) {
            previous
        } else {
            anchor
        };
        self.state.customization.drawer = Some(ContentDrawer {
            anchor,
            columns,
            tool_set: None,
            dismissal: DrawerDismissal::OutsideContact,
            tabs: None,
            compact: false,
        });
        if self.layer_interaction.tool.selection_tool().is_some() { self.refresh_tools(); }
        self.update_slot_drawer();
    }
    pub(crate) fn finish_tool_slot_request(&mut self, previous: LayerCanvasTool) -> bool {
        let changed = self.layer_interaction.tool != previous;
        if changed {
            self.remember_tool_slots();
            if let Some((old, anchor, variant)) = self.pending_tool_drawer.take()
                && variant.is_active(&self.state)
                && self
                    .state
                    .customization
                    .drawer
                    .as_ref()
                    .is_some_and(|d| d.anchor == old)
                && self.state.workspace.layout.anchor_control(anchor).and_then(|c| c.tool_group()).is_some_and(|g| g.contains(variant))
            {
                self.show_tool_slot_drawer(old, anchor);
                return true;
            }
        }
        if !self.content_bounds.busy() {
            self.pending_tool_drawer = None;
        }
        changed && self.update_slot_drawer()
    }
    pub(crate) fn choose_tool_variant(
        &mut self,
        anchor: DrawerAnchor,
        variant: ToolVariant,
    ) -> Result<UiChange, String> {
        let Some(group) = self.state.workspace.layout.anchor_control(anchor).and_then(|c| c.tool_group())
        else {
            return Err(self
                .localization()
                .text(MessageId::WORKSPACE_REFUSAL_CUSTOMIZATION_THE_TARGET_TOOL_NO_LONGER_EXISTS)
                .to_string());
        };
        if !group.contains(variant) {
            return Err(self
                .localization()
                .text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)
                .to_string());
        }
        if let Some(reason) = self.command_disabled_reason(variant.command()) {
            return Err(reason);
        }
        let open = self
            .state
            .customization
            .drawer
            .as_ref()
            .filter(|d| !matches!(d.anchor, DrawerAnchor::Column { .. }))
            .map(|d| d.anchor);
        let previous = self.tool_origin.replace((anchor, group));
        let change = match self.dispatch(self.variant_action(variant)) {
            Ok(change) => change,
            Err(error) => { self.tool_origin = previous; return Err(error); },
        };
        if variant.is_active(&self.state) {
            self.remember_tool_slots();
            if let Some(old) = open {
                self.show_tool_slot_drawer(old, anchor);
            }
        } else if self.content_bounds.busy() {
            self.pending_tool_drawer = open.map(|old| (old, anchor, variant));
        }
        Ok(merge_change(
            change,
            self.changed(
                regions::COMMANDS
                    | regions::BRUSH
                    | if open.is_some() {
                        regions::CUSTOMIZATION
                    } else {
                        0
                    },
                false,
            ),
        ))
    }
    pub(crate) fn activate_tool_group(
        &mut self,
        group: ToolControlGroup,
        anchor: DrawerAnchor,
    ) -> Result<UiChange, String> {
        if self.state.layer_tools.tool.picks_color() {
            return self.dispatch(UiAction::Invoke {
                command: CommandId::Eyedropper,
            });
        }
        let variant = self.group_variant(group);
        if let Some(reason) = self.command_disabled_reason(variant.command()) {
            return Err(reason);
        }
        let selected = variant.is_active(&self.state);
        let open = self.state.customization.drawer.as_ref().map(|d| d.anchor);
        if !selected {
            return self.choose_tool_variant(anchor, variant);
        }
        self.tool_origin = Some((anchor, group));
        self.refresh_tools();
        let action = match anchor {
            DrawerAnchor::Tile { panel, tile } => CustomizationAction::ToggleToolDrawer {
                anchor: TileAnchor { panel, tile },
            },
            DrawerAnchor::Header { id } => CustomizationAction::ToggleHeaderDrawer { id },
            _ => {
                return Err(self
                    .localization()
                    .text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)
                    .to_string());
            }
        };
        let mut change = self.dispatch(UiAction::Customize { action })?;
        change.regions |= regions::BRUSH;
        if open != Some(anchor) && self.update_slot_drawer() {
            change.regions |= regions::CUSTOMIZATION;
        }
        Ok(change)
    }
}
