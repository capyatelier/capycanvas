//! Toolbar control semantics and fitting. Hosts supply native measurements and
//! retain widgets; this module owns bindings, context, ordering and overflow.
use crate::*;
use serde::{Deserialize, Serialize};

impl ToolbarControl {
    pub fn is_component(self) -> bool {
        matches!(
            self,
            Self::BrushSizeSlider | Self::BrushOpacitySlider | Self::ToolOptions
        )
    }
    pub fn slider(self) -> Option<ToolbarNumericBinding> {
        match self {
            Self::BrushSizeSlider => Some(ToolbarNumericBinding::BrushSize),
            Self::BrushOpacitySlider => Some(ToolbarNumericBinding::BrushOpacity),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolbarContext {
    pub generation: u64,
    pub document: u64,
    pub tool: LayerCanvasTool,
    pub brush: u32,
    pub layer: Option<u64>,
    pub mask: bool,
    pub operation: bool,
}

/// Owned projection: native refreshes can dismiss popups and emit focus events
/// without holding a session borrow. Value-only updates retain this schema.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolbarComponentView {
    pub context: ToolbarContext,
    pub numeric: Option<ToolSetting>,
    pub options: Vec<ToolOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ToolbarNumericBinding {
    BrushSize,
    BrushOpacity,
    ToolSetting(String),
}
impl ToolbarNumericBinding {
    pub fn action(&self, value: f32) -> UiAction {
        match self {
            Self::BrushSize => UiAction::SetBrushSize { value },
            Self::BrushOpacity => UiAction::SetBrushOpacity { value },
            Self::ToolSetting(id) => UiAction::SetToolSetting {
                id: id.clone(),
                value,
            },
        }
    }
    pub fn field(&self, state: &UiState) -> Option<ToolSetting> {
        Some(match self {
            Self::BrushSize => ToolSetting {
                id: "size",
                label: "Brush size",
                group: "",
                numeric: NumericControl::brush_size(),
                value: state.brush.diameter,
            },
            Self::BrushOpacity => ToolSetting {
                id: "opacity",
                label: "Brush opacity",
                group: "",
                numeric: NumericControl::percent(),
                value: state.brush.opacity,
            },
            Self::ToolSetting(id) => state.tool_settings.iter().find(|f| f.id == id)?.clone(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ToolOption {
    Numeric(ToolSetting),
    Choice {
        id: &'static str,
        label: &'static str,
        items: Vec<ToolSetItem>,
    },
    Action {
        state: CommandState,
        checkable: bool,
    },
}
impl ToolOption {
    /// Values and selection do not invalidate retained native editors.
    pub fn same_schema(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Numeric(a), Self::Numeric(b)) => {
                a.id == b.id && a.label == b.label && a.numeric == b.numeric
            }
            (
                Self::Choice {
                    id: a, items: x, ..
                },
                Self::Choice {
                    id: b, items: y, ..
                },
            ) => {
                a == b
                    && x.len() == y.len()
                    && x.iter()
                        .zip(y)
                        .all(|(a, b)| a.label == b.label && a.action == b.action)
            }
            (
                Self::Action {
                    state: a,
                    checkable: x,
                },
                Self::Action {
                    state: b,
                    checkable: y,
                },
            ) => a.id == b.id && x == y,
            _ => false,
        }
    }
}

impl UiState {
    pub fn toolbar_component(&self, control: ToolbarControl) -> Option<ToolbarComponentView> {
        control.is_component().then(|| ToolbarComponentView {
            context: self.toolbar_context(),
            numeric: control.slider().and_then(|binding| binding.field(self)),
            options: if control == ToolbarControl::ToolOptions { self.tool_options() } else { Vec::new() },
        })
    }
    pub fn toolbar_context(&self) -> ToolbarContext {
        ToolbarContext {
            generation: self.toolbar_context_generation,
            document: self.document_file.epoch,
            tool: self.layer_tools.tool,
            brush: self.brush.preset,
            layer: self.layer_tools.editing_layer.as_ref().map(|l| l.id),
            mask: self
                .layer_tools
                .editing_layer
                .as_ref()
                .is_some_and(|l| l.mask_selected),
            operation: self
                .tool_actions
                .iter()
                .any(|a| a.command == CommandId::ApplyTransform),
        }
    }
    pub(crate) fn toolbar_edit_allowed(&self, action: &UiAction) -> bool {
        match action {
            UiAction::SetBrushSize { .. } | UiAction::SetBrushOpacity { .. } => true,
            UiAction::SetToolSetting { id, .. } => self.tool_settings.iter().any(|f| f.id == id),
            UiAction::Invoke { command }
                if self.tool_actions.iter().any(|a| a.command == *command) =>
            {
                true
            }
            _ => self
                .tool_set
                .groups
                .iter()
                .chain(&self.tool_set.subtools)
                .any(|i| i.action == *action),
        }
    }
    pub fn tool_options(&self) -> Vec<ToolOption> {
        use CommandId::*;
        let mut options = Vec::new();
        // Completion actions precede values. Overflow always retains access
        // to the complete form, even when neither action fits inline.
        let completion = |c| {
            matches!(
                c,
                ApplyTransform | CancelTransform | CompleteSelection | CancelSelection
            )
        };
        let action = |a: &ToolSettingAction| {
            self.commands
                .iter()
                .find(|c| c.id == a.command)
                .map(|state| ToolOption::Action {
                    state: state.clone(),
                    checkable: a.checkable,
                })
        };
        options.extend(
            self.tool_actions
                .iter()
                .filter(|a| completion(a.command))
                .filter_map(action),
        );
        for (id, label, items) in [
            ("tool", "Tool", &self.tool_set.groups),
            ("variant", "Variant", &self.tool_set.subtools),
        ] {
            if items.len() > 1 && !self.toolbar_context().operation {
                options.push(ToolOption::Choice {
                    id,
                    label,
                    items: items.clone(),
                });
            }
        }
        let modes = [
            SelectionNew,
            SelectionAdd,
            SelectionSubtract,
            SelectionIntersect,
        ];
        let items: Vec<_> = self
            .tool_actions
            .iter()
            .filter(|a| modes.contains(&a.command))
            .filter_map(|a| {
                let c = self.commands.iter().find(|c| c.id == a.command)?;
                Some(ToolSetItem {
                    label: c.label,
                    icon: c.icon.unwrap_or("select"),
                    action: UiAction::Invoke { command: c.id },
                    selected: c.selected,
                    preview: None,
                })
            })
            .collect();
        if !items.is_empty() {
            options.push(ToolOption::Choice {
                id: "selection-mode",
                label: "Mode",
                items,
            });
        }
        options.extend(self.tool_settings.iter().cloned().map(ToolOption::Numeric));
        options.extend(
            self.tool_actions
                .iter()
                .filter(|a| !completion(a.command) && !modes.contains(&a.command))
                .filter_map(action),
        );
        options
    }
}

/// A single row has an explicit grip and an always-reachable complete-options
/// launcher. Fitting uses native natural widths, preserving the shared order.
/// Vertical placement intentionally uses only the launcher.
pub fn tool_options_layout(
    width: f32,
    height: f32,
    axis: Axis,
    widths: &[f32],
) -> (Bounds, Vec<Option<Bounds>>, Bounds) {
    let width = finite_size(width);
    let height = finite_size(height);
    let vertical = axis == Axis::Vertical;
    let grip = if vertical {
        Bounds {
            width,
            height: 12.0_f32.min(height),
            ..Bounds::default()
        }
    } else {
        Bounds {
            width: 12.0_f32.min(width),
            height,
            ..Bounds::default()
        }
    };
    let button_width = 32.0_f32.min((width - grip.width).max(0.0));
    let more = if vertical {
        Bounds {
            y: grip.height,
            width,
            height: (height - grip.height).max(0.0),
            ..Bounds::default()
        }
    } else {
        Bounds {
            x: (width - button_width).max(grip.width),
            width: button_width,
            height,
            ..Bounds::default()
        }
    };
    let mut x = grip.width + 4.0;
    let mut fitting = !vertical;
    let fields = widths
        .iter()
        .map(|w| {
            let w = finite_size(*w);
            fitting &= w > 0.0 && x + w + 4.0 <= more.x;
            if !fitting {
                return None;
            }
            let b = Bounds {
                x,
                y: 0.0,
                width: w,
                height,
            };
            x += w + 8.0;
            Some(b)
        })
        .collect();
    (grip, fields, more)
}

fn finite_size(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Geometry for one compact slider; values open the full native number editor.
pub fn toolbar_slider_layout(width: f32, height: f32, axis: Axis) -> [Bounds; 3] {
    let width = finite_size(width);
    let height = finite_size(height);
    if axis == Axis::Vertical {
        let grip = 12.0_f32.min(height);
        let value = 40.0_f32.min((height - grip).max(0.0));
        let track = height - grip - value;
        [
            Bounds {
                width,
                height: grip,
                ..Bounds::default()
            },
            Bounds {
                y: grip,
                width,
                height: value,
                ..Bounds::default()
            },
            Bounds {
                x: 0.,
                y: grip + value,
                width,
                height: if track >= 32.0 { track } else { 0.0 },
            },
        ]
    } else {
        let grip = 12.0_f32.min(width);
        let value = 64.0_f32.min((width - grip).max(0.));
        let track = width - grip - value;
        [
            Bounds {
                width: grip,
                height,
                ..Bounds::default()
            },
            Bounds {
                x: grip,
                width: value,
                height,
                ..Bounds::default()
            },
            Bounds {
                x: grip + value,
                y: 0.,
                width: if track >= 32.0 { track } else { 0.0 },
                height,
            },
        ]
    }
}
