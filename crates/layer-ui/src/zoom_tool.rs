use crate::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomDrag {
    #[default]
    Smooth,
    Area,
    ClickOnly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomDirection {
    #[default]
    Horizontal,
    Vertical,
}
impl ZoomDirection {
    pub(crate) fn distance(self, from: [f32; 2], to: [f32; 2]) -> f32 {
        match self { Self::Horizontal => to[0] - from[0], Self::Vertical => from[1] - to[1] }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ZoomToolSettings {
    pub zoom_out: bool,
    pub drag: ZoomDrag,
    pub direction: ZoomDirection,
    pub center_clicked_point: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ZoomToolAction {
    Click { out: bool },
    Drag { value: ZoomDrag },
    Direction { value: ZoomDirection },
}
impl ZoomToolSettings {
    pub(crate) fn apply(&mut self, action: ZoomToolAction) {
        match action {
            ZoomToolAction::Click { out } => self.zoom_out = out,
            ZoomToolAction::Drag { value } => self.drag = value,
            ZoomToolAction::Direction { value } => self.direction = value,
        }
    }
    pub(crate) fn controls(self, localizer: &Localizer) -> Vec<ToolOption> {
        let choice = |id, label, entries: &[(MessageId, ZoomToolAction, bool, &'static str)]| ToolOption::Choice {
            id, label: localizer.text(label), segmented: true, labeled: true, columns: None, beside: None,
            items: entries.iter().map(|&(label, action, selected, icon)| ToolSetItem {
                enabled: true, label: localizer.text(label), icon, selected,
                action: UiAction::ZoomTool { action }, preview: None,
            }).collect(),
        };
        vec![
            choice("zoom-click", MessageId::ZOOM_TOOL_CLICK, &[
                (MessageId::ZOOM_TOOL_IN, ZoomToolAction::Click {out:false}, !self.zoom_out, "plus"),
                (MessageId::ZOOM_TOOL_OUT, ZoomToolAction::Click {out:true}, self.zoom_out, "minus"),
            ]),
            choice("zoom-drag", MessageId::ZOOM_TOOL_DRAG, &[
                (MessageId::ZOOM_TOOL_SMOOTH, ZoomToolAction::Drag {value:ZoomDrag::Smooth}, self.drag==ZoomDrag::Smooth, "plus"),
                (MessageId::ZOOM_TOOL_AREA, ZoomToolAction::Drag {value:ZoomDrag::Area}, self.drag==ZoomDrag::Area, "select"),
                (MessageId::ZOOM_TOOL_CLICK_ONLY, ZoomToolAction::Drag {value:ZoomDrag::ClickOnly}, self.drag==ZoomDrag::ClickOnly, "search"),
            ]),
            choice("zoom-direction", MessageId::ZOOM_TOOL_DIRECTION, &[
                (MessageId::ZOOM_TOOL_LEFT_RIGHT, ZoomToolAction::Direction {value:ZoomDirection::Horizontal}, self.direction==ZoomDirection::Horizontal, "plus"),
                (MessageId::ZOOM_TOOL_UP_DOWN, ZoomToolAction::Direction {value:ZoomDirection::Vertical}, self.direction==ZoomDirection::Vertical, "plus"),
            ]),
        ]
    }
}
