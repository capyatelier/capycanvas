//! Toolbar control semantics and fitting. Hosts supply native measurements and
//! retain widgets; this module owns bindings, context, ordering and overflow.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolOptionsStyle {
    pub text: bool,
    pub sliders: bool,
}
impl Default for ToolOptionsStyle {
    fn default() -> Self {
        Self {
            text: true,
            sliders: true,
        }
    }
}

impl ToolbarControl {
    pub const TOOL_OPTIONS: Self = Self::ToolOptions {
        style: ToolOptionsStyle {
            text: true,
            sliders: true,
        },
    };
    pub fn options_style(self) -> Option<ToolOptionsStyle> {
        match self {
            Self::ToolOptions { style } => Some(style),
            _ => None,
        }
    }

    pub fn is_component(self) -> bool {
        matches!(
            self,
            Self::BrushSizeSlider | Self::BrushOpacitySlider | Self::ToolOptions { .. }
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
    pub bookmarks: Vec<SliderBookmark>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ToolbarNumericBinding {
    BrushSize,
    BrushOpacity,
}
impl ToolbarNumericBinding {
    pub fn numeric(&self) -> NumericControl {
        match self {
            Self::BrushSize => NumericControl::brush_size(),
            Self::BrushOpacity => NumericControl {
                digits: 0,
                resolution: 0.01,
                ..NumericControl::percent()
            },
        }
    }
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
        state
            .tool_settings
            .iter()
            .find(|f| f.id == id)
            .cloned()
            .map(|mut field| {
                field.numeric = self.numeric();
                field
            })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[expect(clippy::large_enum_variant, reason = "Tool option publications retain inline range fields without allocating")]
pub enum ToolOption {
    Gradient(Box<PropertyControl>),
    Numeric(ToolSetting),
    /// One atomic interval field; endpoint edits use the existing setting IDs.
    Range {
        id: &'static str,
        label: std::sync::Arc<str>,
        bounds: [ToolSetting; 2],
    },
    Choice {
        id: &'static str,
        label: std::sync::Arc<str>,
        segmented: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        columns: Option<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        beside: Option<&'static str>,
        items: Vec<ToolSetItem>,
    },
    Action {
        state: CommandState,
        checkable: bool,
    },
}

impl ToolOption {
    /// Values, selection and presentation copy do not invalidate native editors.
    pub fn same_schema(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Gradient(a),Self::Gradient(b))=>a.key==b.key && a.gradient.as_ref().map(|c|&c.destination)==b.gradient.as_ref().map(|c|&c.destination),
            (Self::Numeric(a), Self::Numeric(b)) => {
                a.id == b.id && a.numeric == b.numeric
            }
            (Self::Range { id: a, bounds: p, .. }, Self::Range { id: b, bounds: q, .. }) => {
                a == b && p.iter().zip(q.iter()).all(|(a, b)| {
                    a.id == b.id && a.numeric == b.numeric
                })
            }
            (
                Self::Choice {
                    id: a,
                    segmented: p,
                    columns: c,
                    beside: e,
                    items: x,
                    ..
                },
                Self::Choice {
                    id: b,
                    segmented: q,
                    columns: d,
                    beside: f,
                    items: y,
                    ..
                },
            ) => {
                a == b
                    && p == q && c == d && e == f
                    && x.len() == y.len()
                    && x.iter()
                        .zip(y)
                        .all(|(a, b)| a.action == b.action)
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

impl CommandState {
    pub(crate) fn choice_item(&self, label: std::sync::Arc<str>) -> ToolSetItem {
        ToolSetItem { enabled: self.enabled,
            label,
            icon: self.icon.unwrap_or("select"),
            action: UiAction::Invoke { command: self.id },
            selected: self.selected,
            preview: None,
        }
    }
}

impl UiState {
    pub fn toolbar_component(&self, control: ToolbarControl) -> Option<ToolbarComponentView> {
        control.is_component().then(|| ToolbarComponentView {
            context: self.toolbar_context(),
            numeric: control.slider().and_then(|binding| binding.field(self)),
            bookmarks: control.slider().map_or_else(Vec::new, |binding| {
                let current = binding.field(self).map(|f| f.value);
                self.settings.slider_bookmarks.get(&self.brush.preset.to_string())
                    .map_or_else(Vec::new, |marks| marks.view(&binding, current))
            }),
            options: if control.options_style().is_some() {
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
            UiAction::Effect {action} => {
                let action=if let crate::EffectAction::Gesture {action,..}=action {action.as_ref()} else {action};
                matches!(action,crate::EffectAction::Gradient {target,..} if self.tool_extra.iter().any(|option|
                    matches!(option,ToolOption::Gradient(control) if control.gradient.as_ref().is_some_and(|gradient|&gradient.destination==target))))
            }
            UiAction::Tonal { .. } | UiAction::TransformReference { .. } => self.tool_extra.iter().any(|o| matches!(o,ToolOption::Choice {items,..} if items.iter().any(|i| i.action==*action))),
            UiAction::ToggleSliderBookmark { control } => control.slider()
                .is_some_and(|binding| binding.field(self).is_some()),
            UiAction::SetBrushSize { .. } => {
                self.layer_tools.tool == LayerCanvasTool::Paint && !self.toolbar_context().operation
            }
            UiAction::SetBrushOpacity { .. } => {
                self.layer_tools.tool == LayerCanvasTool::Paint && !self.toolbar_context().operation
            }
            UiAction::SetToolSetting { id, .. } | UiAction::ResetToolSetting { id } => {
                self.tool_settings.iter().any(|f| f.id == id)
            }
            UiAction::Invoke { command }
                if self.tool_actions.iter().any(|a| a.command == *command) =>
            {
                true
            }
            _ => {
                self.tool_set
                    .groups
                    .iter()
                    .chain(&self.tool_set.subtools)
                    .any(|i| i.action == *action)
                    || self.tool_options().iter().chain(&self.tool_extra).any(|option| {
                        matches!(option, ToolOption::Choice { items, .. } if items.iter().any(|i| i.action == *action))
                    })
            }
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
        let choice = |id, label: std::sync::Arc<str>, segmented, items: Vec<ToolSetItem>| {
            (items.len() > 1).then_some(ToolOption::Choice {
                id,
                label,
                segmented,
                columns: None,
                beside: None,
                items,
            })
        };
        if !self.toolbar_context().operation {
            options.extend(choice("tool", self.localization.text(MessageId::TOOLBAR_TOOL), false, self.tool_set.groups.clone()));
            let (samples, variants): (Vec<_>, Vec<_>) = self
                .tool_set
                .subtools
                .iter()
                .cloned()
                .partition(|i| matches!(i.action, UiAction::SetColorSampleSize { .. }));
            let label = if self.layer_tools.tool.picks_color()
                || matches!(self.layer_tools.tool, LayerCanvasTool::Region { .. })
            {
                self.localization.text(MessageId::TOOLBAR_SOURCE)
            } else {
                self.localization.text(MessageId::TOOLBAR_VARIANT)
            };
            if self.layer_tools.tool.picks_color() {
                if !self.color_picker.calibrating { options.extend(choice("picker-style", self.localization.text(MessageId::TOOLBAR_STYLE), false, variants)); }
                let sources = [(MessageId::TOOLBAR_VISIBLE_COLOR, false), (MessageId::TOOLBAR_SELECTED_LAYER, true)].into_iter()
                    .filter(|(_, layer)| !layer || self.color_picker.can_sample_layer)
                    .map(|(label, layer)| ToolSetItem { enabled: true, label: self.localization.text(label), icon: if layer { "layers" } else { "eye" },
                        action: UiAction::ColorPicker { action: crate::ColorPickerAction::Source { layer } },
                        selected: self.color_picker.layer == layer, preview: None }).collect();
                if !self.color_picker.calibrating { options.extend(choice("variant", self.localization.text(MessageId::TOOLBAR_SOURCE), false, sources)); }
                let sizes = [(MessageId::TOOLBAR_SINGLE_PIXEL,1),(MessageId::TOOLBAR_5_PX_CIRCLE,5),(MessageId::TOOLBAR_15_PX_CIRCLE,15),(MessageId::TOOLBAR_51_PX_CIRCLE,51),(MessageId::TOOLBAR_101_PX_CIRCLE,101)].into_iter()
                    .filter(|(_,width)|self.color_picker.sample_sizes.contains(width))
                    .map(|(label,width)| ToolSetItem { enabled: true, label: self.localization.text(label), icon: "eyedropper", action: UiAction::SetColorSampleSize { width },
                        selected: self.color_picker.sample_width == width, preview: None }).collect();
                options.extend(choice("sample-size", self.localization.text(MessageId::TOOLBAR_SAMPLE_SIZE), false, sizes));
            } else {
                options.extend(choice("variant", label, false, variants));
                options.extend(choice("sample-size", self.localization.text(MessageId::TOOLBAR_SAMPLE_SIZE), false, samples));
            }
        }
        for group in [
            ToolActionGroup::TransformMode,
            ToolActionGroup::TransformInterpolation,
            ToolActionGroup::TransformWarpGrid,
            ToolActionGroup::SelectionMode,
            ToolActionGroup::SelectionSource,
            ToolActionGroup::ColorMixing,
        ] {
            let items = self
                .tool_actions
                .iter()
                .filter(|a| a.group() == Some(group))
                .filter_map(|a| self.commands.iter().find(|c| c.id == a.command))
                .map(|c| c.choice_item(c.label.clone()))
                .collect();
            options.extend(choice(group.id(), group.localized_label(&self.localization), group.segmented(), items));
        }
        options.extend(self.tool_extra.iter().filter(|option| !matches!(option,ToolOption::Choice {id:"gradient-shape",..})).cloned());
        let mut fields = self.tool_settings.iter().peekable();
        while let Some(field) = fields.next() {
            if field.id == "tonal_lower" && fields.peek().is_some_and(|f| f.id == "tonal_upper") {
                options.push(ToolOption::Range {
                    id: "tonal",
                    label: self.localization.text(MessageId::TOOLBAR_RANGE_IN_STOPS_RELATIVE_TO_REFERENCE_WHITE_0),
                    bounds: [field.clone(), fields.next().unwrap().clone()],
                });
            } else {
                options.push(ToolOption::Numeric(field.clone()));
            }
        }
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
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
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
    let more = Bounds {
        x: (width - finite_size(button[0])).max(0.),
        y: (height - finite_size(button[1])).max(0.),
        width: finite_size(button[0]).min(width),
        height: finite_size(button[1]).min(height),
    };
    let mut fields = vec![None; sizes.len()];
    let (mut start, mut y) = (0, 0.);
    while start < sizes.len() {
        let (mut end, mut x) = (start, 0.);
        let mut row_height = finite_size(button[1]);
        // Measure each row separately: a stacked segmented choice must not
        // stretch every other row to the height of its whole button bar.
        while end < sizes.len() {
            let [w, h] = sizes[end].map(finite_size);
            if w == 0. || h == 0. || w > width {
                break;
            }
            let w = if vertical && width < w * 2. + gap {
                width
            } else {
                w
            };
            if x + w > width {
                break;
            }
            fields[end] = Some(Bounds {
                x,
                y,
                width: w,
                height: h,
            });
            row_height = row_height.max(h);
            x += w + gap;
            end += 1;
        }
        if end == start {
            break;
        }
        for i in start..end {
            let b = fields[i].as_mut().unwrap();
            b.height = row_height;
            if y + row_height > height
                || (b.y + b.height + gap > more.y && b.x + b.width + gap > more.x)
            {
                fields[i..].fill(None);
                return ToolOptionsLayout { fields, more };
            }
        }
        start = end;
        y += row_height + gap;
    }
    ToolOptionsLayout { fields, more }
}

fn finite_size(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Equal end insets around the editable track. The empty leading inset remains
/// a hold-to-reorder target without reserving space for a numeric readout.
pub fn toolbar_slider_layout(width: f32, height: f32, axis: Axis) -> [Bounds; 2] {
    let width = finite_size(width);
    let height = finite_size(height);
    let vertical = axis == Axis::Vertical;
    let cap = 8_f32.min(if vertical { height } else { width } / 2.);
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
                height: height - 2. * cap,
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
                width: width - 2. * cap,
                height,
                ..Bounds::default()
            },
        ]
    }
}

/// Icons belong to the shared field schema, including compact native hosts.
pub fn tool_setting_icon(id: &str) -> &'static str {
    match id {
        "size" => "brush-size",
        "size_jitter" => "size",
        "opacity" => "opacity",
        "flow" => "paint-flow",
        "hardness" => "hardness",
        "spacing" => "brush-spacing",
        "distance" => "ruler",
        "angle" | "transform_angle" => "angle",
        "rotation_jitter" => "rotation-variation",
        "grain_depth" | "bristle_scale" => "grain",
        "paint" | "bristle_load" => "paint-load",
        "water_load" => "water",
        "pull" => "eyedropper",
        "dilution" => "dilution",
        "wet_edge" => "edge-strength",
        "edge_width" => "edge-width",
        "wet_flow" => "wet-bleed",
        "dry_flow" => "dry-bleed",
        "transform_width" | "selection_width" | "selection_ratio_width" => "width",
        "transform_height" | "selection_height" | "selection_ratio_height" => "height",
        "transform_x" => "position-x",
        "transform_y" => "position-y",
        "selection_feather" => "feather",
        "smoothing" => "edge-smooth",
        "gap_closing" => "close-gap",
        "expansion" => "expand",
        "strength" => "strength",
        "distortion" => "distort",
        "momentum" => "motion-blur",
        "tolerance" => "color-select",
        _ => "settings",
    }
}
