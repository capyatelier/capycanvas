use super::*;
use std::collections::BTreeMap;

variants! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ToolSlotId {
        Drawing, Marquee, Lasso, AutomaticSelection, ManualSelection, Healing,
        PhotoFill, Fill, Blend, Operation, Figure, Ruler, Gradient,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolVariant {
    Command { command: CommandId },
    Figure { shape: FigureShape },
    Ruler { kind: RulerKind },
    Gradient { radial: bool, transparent: bool },
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
                        V::Gradient {
                            radial: false,
                            transparent: false,
                        },
                        V::Gradient {
                            radial: false,
                            transparent: true,
                        },
                        V::Gradient {
                            radial: true,
                            transparent: false,
                        },
                        V::Gradient {
                            radial: true,
                            transparent: true,
                        },
                        command(C::Fill),
                        command(C::LassoFill),
                    ]
                }
            }
            Self::Fill => const { &[command(C::Fill), command(C::LassoFill)] },
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
                        V::Gradient {
                            radial: false,
                            transparent: false,
                        },
                        V::Gradient {
                            radial: false,
                            transparent: true,
                        },
                        V::Gradient {
                            radial: true,
                            transparent: false,
                        },
                        V::Gradient {
                            radial: true,
                            transparent: true,
                        },
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
            Self::Figure { .. } => CommandId::Figure,
            Self::Ruler { .. } => CommandId::Ruler,
            Self::Gradient { .. } => CommandId::Gradient,
        }
    }
    pub(crate) fn control(self) -> ToolbarControl {
        ToolbarControl::Command {
            command: self.command(),
        }
    }
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Command { command } => command.icon().unwrap_or("menu"),
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
            Self::Gradient {
                radial,
                transparent,
            } => match (radial, transparent) {
                (false, false) => "gradient",
                (false, true) => "gradient-transparent",
                (true, false) => "gradient-radial",
                (true, true) => "gradient-radial-transparent",
            },
        }
    }
    fn label(self, localization: &Localizer) -> String {
        let id = match self {
            Self::Command { command } => return command.localized_label(localization).to_string(),
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
            Self::Gradient {
                radial: false,
                transparent: false,
            } => MessageId::TOOL_GRADIENT_LINEAR_COLOR,
            Self::Gradient {
                radial: false,
                transparent: true,
            } => MessageId::TOOL_GRADIENT_LINEAR_CLEAR,
            Self::Gradient {
                radial: true,
                transparent: false,
            } => MessageId::TOOL_GRADIENT_RADIAL_COLOR,
            Self::Gradient {
                radial: true,
                transparent: true,
            } => MessageId::TOOL_GRADIENT_RADIAL_CLEAR,
        };
        localization.text(id).to_string()
    }
    fn active(state: &UiState) -> Option<Self> {
        let tool = state.layer_tools.tool;
        Some(match tool {
            LayerCanvasTool::Figure { shape, .. } => Self::Figure { shape },
            LayerCanvasTool::Ruler { kind } => Self::Ruler { kind },
            LayerCanvasTool::Gradient {
                radial,
                transparent,
            } => Self::Gradient {
                radial,
                transparent,
            },
            LayerCanvasTool::Paint => command(state.brush.tool.command()),
            _ => command(if let Some(selection) = tool.selection_tool() {
                selection.command()
            } else {
                match tool {
                    LayerCanvasTool::Move => CommandId::Move,
                    LayerCanvasTool::Transform => CommandId::ScaleRotate,
                    LayerCanvasTool::Region { fill: true, .. } => CommandId::Fill,
                    LayerCanvasTool::LassoFill => CommandId::LassoFill,
                    LayerCanvasTool::Crop => CommandId::Crop,
                    LayerCanvasTool::Hand => CommandId::Hand,
                    _ => return None,
                }
            }),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSlotSelection {
    pub slot: ToolSlotId,
    pub variant: ToolVariant,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSlotMemory {
    pub tiles: BTreeMap<u32, ToolSlotSelection>,
    pub headers: BTreeMap<u32, ToolSlotSelection>,
}
impl ToolSlotMemory {
    fn get(&self, anchor: DrawerAnchor, slot: ToolSlotId) -> Option<ToolVariant> {
        let choice = match anchor {
            DrawerAnchor::Tile { tile, .. } => self.tiles.get(&tile),
            DrawerAnchor::Header { id } => self.headers.get(&id),
            _ => None,
        }?;
        (choice.slot == slot && slot.variants().contains(&choice.variant)).then_some(choice.variant)
    }
    fn remember(&mut self, anchor: DrawerAnchor, slot: ToolSlotId, variant: ToolVariant) {
        let (map, id) = match anchor {
            DrawerAnchor::Tile { tile, .. } => (&mut self.tiles, tile),
            DrawerAnchor::Header { id } => (&mut self.headers, id),
            _ => return,
        };
        map.insert(id, ToolSlotSelection { slot, variant });
    }
    pub(crate) fn retain_history(&mut self, history: &LayoutHistory) {
        let slots: Vec<_> = history
            .revisions
            .values()
            .flat_map(|r| r.layout.tool_slots())
            .collect();
        self.tiles.retain(|id, choice| {
            slots.iter().any(|(a, slot)| {
                matches!(a, DrawerAnchor::Tile {tile,..} if tile==id)
                    && *slot == choice.slot
                    && slot.variants().contains(&choice.variant)
            })
        });
        self.headers.retain(|id, choice| {
            slots.iter().any(|(a, slot)| {
                matches!(a, DrawerAnchor::Header {id:header} if header==id)
                    && *slot == choice.slot
                    && slot.variants().contains(&choice.variant)
            })
        });
    }
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
                if let ToolbarControl::ToolSlot { slot } = control {
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
    pub(crate) fn slot_variant(&self, slot: ToolSlotId, anchor: DrawerAnchor) -> ToolVariant {
        ToolVariant::active(self)
            .filter(|v| slot.variants().contains(v))
            .or_else(|| self.tool_slots.get(anchor, slot))
            .unwrap_or(slot.variants()[0])
    }
    pub(crate) fn resolve_slot(
        &self,
        slot: ToolSlotId,
        anchor: DrawerAnchor,
    ) -> (ToolChoice, bool, String, ToolbarControl) {
        let variant = self.slot_variant(slot, anchor);
        let mut choice = tool_choice_localized(variant.control(), &self.localization);
        choice.control = ToolbarControl::ToolSlot { slot };
        choice.label = variant.label(&self.localization);
        choice.icon = variant.icon();
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
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(crate) fn variant_action(&self, variant: ToolVariant) -> UiAction {
        match variant {
            ToolVariant::Command { command } => UiAction::Invoke { command },
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
            ToolVariant::Gradient {
                radial,
                transparent,
            } => UiAction::Layer {
                action: LayerAction::Tool {
                    tool: LayerCanvasTool::Gradient {
                        radial,
                        transparent,
                    },
                },
            },
        }
    }
    pub(crate) fn tool_variants_menu(&self, anchor: DrawerAnchor) -> Result<ContextMenu, String> {
        let Some(ToolbarControl::ToolSlot { slot }) =
            self.state.workspace.layout.anchor_control(anchor)
        else {
            return Err(self
                .localization()
                .text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)
                .to_string());
        };
        let remembered = self.state.slot_variant(slot, anchor);
        let items = slot
            .variants()
            .iter()
            .filter(|v| v.command().available_on(self.state.platform))
            .map(|&variant| {
                let mut item = ContextMenuItem::command(
                    variant.label(self.localization()),
                    UiAction::ChooseToolVariant { anchor, variant },
                );
                item.icon = Some(variant.icon());
                item.selected = Some(remembered == variant);
                item.enabled = self.command(variant.command()).enabled;
                if let Some(reason) = self.command_disabled_reason(variant.command()) {
                    item.hint = reason;
                }
                item
            })
            .collect();
        Ok(ContextMenu {
            title: slot.label(self.localization()),
            sections: vec![items],
        })
    }
    pub(crate) fn remember_tool_slots(&mut self) {
        if self.interaction.applying_hold
            || self.interaction.hold_base.is_some()
            || self.interaction.spring.is_some()
            || self.interaction.restores.iter().any(|restore| matches!(restore, crate::interaction::Restore::Tool(..)))
            || self.layer_interaction.tool.picks_color()
        {
            return;
        }
        let Some(variant) = ToolVariant::active(&self.state) else {
            return;
        };
        for (anchor, slot) in self.state.workspace.layout.tool_slots() {
            if slot.variants().contains(&variant) {
                self.state.tool_slots.remember(anchor, slot, variant);
            }
        }
    }
    fn slot_drawer_tools(&self, slot: ToolSlotId, anchor: DrawerAnchor) -> ToolSetView {
        let active = ToolVariant::active(&self.state);
        let groups = slot
            .variants()
            .iter()
            .filter(|v| v.command().available_on(self.state.platform))
            .map(|&variant| ToolSetItem {
                enabled: self.command(variant.command()).enabled,
                label: variant.label(self.localization()).into(),
                icon: variant.icon(),
                action: UiAction::ChooseToolVariant { anchor, variant },
                selected: active == Some(variant),
                preview: None,
            })
            .collect();
        let subtools = match self.state.layer_tools.tool {
            LayerCanvasTool::Paint => self
                .state
                .tool_set
                .groups
                .iter()
                .chain(&self.state.tool_set.subtools)
                .cloned()
                .collect(),
            LayerCanvasTool::Figure { .. } | LayerCanvasTool::Region { fill: true, .. } => {
                self.state.tool_set.subtools.clone()
            }
            _ => Vec::new(),
        };
        ToolSetView { groups, subtools }
    }
    pub(crate) fn update_slot_drawer(&mut self) -> bool {
        let Some(drawer) = self.state.customization.drawer.as_ref() else {
            return false;
        };
        let anchor = drawer.anchor;
        let control = self.state.workspace.layout.anchor_control(anchor);
        let active = ToolVariant::active(&self.state);
        let slot = match control {
            Some(ToolbarControl::ToolSlot { slot }) => Some((anchor, slot)),
            Some(ToolbarControl::ToolOptions { .. }) => self
                .state
                .workspace
                .layout
                .tool_slots()
                .find(|(_, slot)| active.is_some_and(|v| slot.variants().contains(&v))),
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
                Some(self.slot_drawer_tools(slot, origin)),
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
                Some(ToolbarControl::ToolSlot { slot }) => {
                    ToolVariant::active(&self.state).is_some_and(|v| slot.variants().contains(&v))
                }
                _ => false,
            }
        })
    }
    pub(crate) fn seed_tool_slots(&mut self, before: &DockLayout) {
        for (anchor, slot) in self.state.workspace.layout.tool_slots() {
            if self.state.tool_slots.get(anchor, slot).is_none()
                && !before.tool_slots().any(|(old, _)| old == anchor)
                && let Some(variant) = before
                    .tool_slots()
                    .filter(|(_, old)| *old == slot)
                    .find_map(|(old, _)| self.state.tool_slots.get(old, slot))
            {
                self.state.tool_slots.remember(anchor, slot, variant);
            }
        }
    }
    fn show_tool_slot_drawer(&mut self, previous: DrawerAnchor, anchor: DrawerAnchor) {
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
            columns: vec![vec![Panel::Brushes], vec![Panel::ToolSettings]],
            tool_set: None,
            dismissal: DrawerDismissal::OutsideContact,
            tabs: None,
            compact: false,
        });
        self.update_slot_drawer();
    }
    pub(crate) fn finish_tool_slot_request(&mut self, previous: LayerCanvasTool) -> bool {
        let changed = self.layer_interaction.tool != previous;
        if changed {
            self.remember_tool_slots();
            if let Some((old, anchor, variant)) = self.pending_tool_drawer.take()
                && ToolVariant::active(&self.state) == Some(variant)
                && self
                    .state
                    .customization
                    .drawer
                    .as_ref()
                    .is_some_and(|d| d.anchor == old)
                && matches!(self.state.workspace.layout.anchor_control(anchor),Some(ToolbarControl::ToolSlot {slot}) if slot.variants().contains(&variant))
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
        let Some(ToolbarControl::ToolSlot { slot }) =
            self.state.workspace.layout.anchor_control(anchor)
        else {
            return Err(self
                .localization()
                .text(MessageId::WORKSPACE_REFUSAL_CUSTOMIZATION_THE_TARGET_TOOL_NO_LONGER_EXISTS)
                .to_string());
        };
        if !slot.variants().contains(&variant) {
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
        let change = self.dispatch(self.variant_action(variant))?;
        if ToolVariant::active(&self.state) == Some(variant) {
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
    pub(crate) fn activate_tool_slot(
        &mut self,
        slot: ToolSlotId,
        anchor: DrawerAnchor,
    ) -> Result<UiChange, String> {
        if self.state.layer_tools.tool.picks_color() {
            return self.dispatch(UiAction::Invoke {
                command: CommandId::Eyedropper,
            });
        }
        let variant = self.state.slot_variant(slot, anchor);
        if let Some(reason) = self.command_disabled_reason(variant.command()) {
            return Err(reason);
        }
        let selected = ToolVariant::active(&self.state) == Some(variant);
        let open = self.state.customization.drawer.as_ref().map(|d| d.anchor);
        if !selected {
            return self.choose_tool_variant(anchor, variant);
        }
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
        if open != Some(anchor) && self.update_slot_drawer() {
            change.regions |= regions::CUSTOMIZATION;
        }
        Ok(change)
    }
}
