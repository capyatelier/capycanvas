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
}
impl ToolbarNumericBinding {
    pub fn action(&self, value: f32) -> UiAction {
        match self {
            Self::BrushSize => UiAction::SetToolSetting {
                id: "size".into(),
                value,
            },
            Self::BrushOpacity => UiAction::SetToolSetting {
                id: "opacity".into(),
                value,
            },
        }
    }
    pub fn field(&self, state: &UiState) -> Option<ToolSetting> {
        let id = match self {
            Self::BrushSize => "size",
            Self::BrushOpacity => "opacity",
        };
        state.tool_settings.iter().find(|f| f.id == id).cloned()
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
            options: if control == ToolbarControl::ToolOptions {
                self.tool_options()
            } else {
                Vec::new()
            },
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
            UiAction::SetBrushSize { .. } => {
                self.layer_tools.tool == LayerCanvasTool::Paint && !self.toolbar_context().operation
            }
            UiAction::SetBrushOpacity { .. } => {
                self.layer_tools.tool == LayerCanvasTool::Paint && !self.toolbar_context().operation
            }
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
        let choice = |id, label, items: Vec<ToolSetItem>| {
            (items.len() > 1).then_some(ToolOption::Choice { id, label, items })
        };
        if !self.toolbar_context().operation {
            options.extend(choice("tool", "Tool", self.tool_set.groups.clone()));
            let (samples, variants): (Vec<_>, Vec<_>) = self
                .tool_set
                .subtools
                .iter()
                .cloned()
                .partition(|i| matches!(i.action, UiAction::SetColorSampleSize { .. }));
            let label = if self.layer_tools.tool.picks_color()
                || matches!(self.layer_tools.tool, LayerCanvasTool::Region { .. })
            {
                "Source"
            } else {
                "Variant"
            };
            options.extend(choice("variant", label, variants));
            options.extend(choice("sample-size", "Sample size", samples));
        }
        for group in [
            ToolActionGroup::SelectionMode,
            ToolActionGroup::SelectionSource,
        ] {
            let items = self
                .tool_actions
                .iter()
                .filter(|a| a.group() == Some(group))
                .filter_map(|a| self.commands.iter().find(|c| c.id == a.command))
                .map(|c| ToolSetItem {
                    label: c.label,
                    icon: c.icon.unwrap_or("select"),
                    action: UiAction::Invoke { command: c.id },
                    selected: c.selected,
                    preview: None,
                })
                .collect();
            options.extend(choice(group.id(), group.label(), items));
        }
        options.extend(self.tool_settings.iter().cloned().map(ToolOption::Numeric));
        options.extend(
            self.tool_actions
                .iter()
                .filter(|a| !completion(a.command) && a.group().is_none())
                .filter_map(action),
        );
        options
    }
}

/// Native natural sizes and theme spacing are supplied by the host. The core
/// chooses a contiguous prefix and always reserves access to the complete form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolOptionsLayout {
    pub fields: Vec<Option<Bounds>>,
    pub more: Bounds,
}
pub fn tool_options_layout(
    width: f32,
    height: f32,
    axis: Axis,
    sizes: &[[f32; 2]],
    button: [f32; 2],
    gap: f32,
) -> ToolOptionsLayout {
    let width = finite_size(width);
    let height = finite_size(height);
    let gap = finite_size(gap);
    let vertical = axis == Axis::Vertical;
    let more = if vertical {
        let h = finite_size(button[1]).min(height);
        Bounds {
            y: height - h,
            width,
            height: h,
            ..Bounds::default()
        }
    } else {
        let w = finite_size(button[0]).min(width);
        Bounds {
            x: width - w,
            width: w,
            height,
            ..Bounds::default()
        }
    };
    let mut offset = 0.0;
    let mut fitting = true;
    let fields = sizes
        .iter()
        .map(|size| {
            let w = finite_size(size[0]);
            let h = finite_size(size[1]);
            fitting &= w > 0.0
                && h > 0.0
                && if vertical {
                    w <= width && offset + h + gap <= more.y
                } else {
                    h <= height && offset + w + gap <= more.x
                };
            if !fitting {
                return None;
            }
            let b = if vertical {
                Bounds {
                    x: 0.,
                    y: offset,
                    width,
                    height: h,
                }
            } else {
                Bounds {
                    x: offset,
                    y: 0.,
                    width: w,
                    height,
                }
            };
            offset += if vertical { h + gap } else { w + gap };
            Some(b)
        })
        .collect();
    ToolOptionsLayout { fields, more }
}

fn finite_size(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Measured value cap followed by the directly editable track. The cap is also
/// the hold-to-reorder target; no separate grip consumes slider space.
pub fn toolbar_slider_layout(width: f32, height: f32, axis: Axis, cap: f32) -> [Bounds; 2] {
    let width = finite_size(width);
    let height = finite_size(height);
    let vertical = axis == Axis::Vertical;
    let cap = finite_size(cap).min(if vertical { height } else { width });
    if vertical {
        [
            Bounds {
                width,
                height: cap,
                ..Bounds::default()
            },
            Bounds {
                y: cap,
                width,
                height: height - cap,
                ..Bounds::default()
            },
        ]
    } else {
        [
            Bounds {
                width: cap,
                height,
                ..Bounds::default()
            },
            Bounds {
                x: cap,
                width: width - cap,
                height,
                ..Bounds::default()
            },
        ]
    }
}
