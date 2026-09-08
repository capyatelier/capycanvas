use crate::renderer::Renderer;
use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_ui::{ContactPhase, PointerButton, PointerKind, UiAction, UiInput, UiSession};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) struct App {
    pub session: UiSession<Renderer>,
    pub logical: [f32; 2],
    pub dirty: bool,
    pub chrome_hidden: bool,
    pub error: Option<String>,
    pub sequence: u64,
    #[cfg(target_os = "android")]
    pub profiling: bool,
    #[cfg(target_os = "android")]
    pub frame_cost: [i64; 3],
    last_pen: Option<PenEvent>,
    #[cfg(target_os = "android")]
    pub surface: Option<crate::android::Surface>,
    #[cfg(target_os = "android")]
    pub instance: Option<wgpu::Instance>,
}

impl App {
    pub fn new() -> Result<Self, String> {
        let mut session = UiSession::blank(Renderer::default(), [1, 1])?;
        session.set_platform(layer_ui::Platform::Android);
        Ok(Self {
            session,
            logical: [1.0, 1.0],
            dirty: true,
            chrome_hidden: false,
            error: None,
            sequence: 0,
            #[cfg(target_os = "android")]
            profiling: false,
            #[cfg(target_os = "android")]
            frame_cost: [0; 3],
            last_pen: None,
            #[cfg(target_os = "android")]
            surface: None,
            #[cfg(target_os = "android")]
            instance: None,
        })
    }
    pub fn resize(&mut self, width: u32, height: u32, density: f32) -> Result<(), String> {
        if width == 0 || height == 0 || !density.is_finite() || density <= 0.0 {
            return Err("Invalid Android surface dimensions".into());
        }
        self.logical = [width as f32 / density, height as f32 / density];
        self.session.set_viewport(self.logical, [width, height])?;
        self.dirty = true;
        Ok(())
    }
    pub fn dispatch(&mut self, action: UiAction) -> Result<(), String> {
        self.dirty |= self.session.dispatch(action)?.canvas_wake;
        Ok(())
    }
    pub fn input(&mut self, input: UiInput) -> Result<layer_ui::InputReply, String> {
        let reply = self.session.input(input)?;
        self.chrome_hidden = reply.chrome_hidden;
        self.dirty |= reply.change.canvas_wake;
        if reply.cancel_paint {
            self.cancel_pen()?;
        }
        Ok(reply)
    }
    fn cancel_pen(&mut self) -> Result<(), String> {
        if let Some(mut event) = self.last_pen.take() {
            event.phase = PenPhase::Cancel;
            self.sequence += 1;
            event.sequence = self.sequence;
            self.enqueue(event)?;
        }
        Ok(())
    }
    fn enqueue(&mut self, event: PenEvent) -> Result<(), String> {
        if let Err(event) = self.session.pen(event) {
            // The sole render owner may drain a full input queue. The Android
            // UI thread never waits here, and a stroke boundary is never dropped.
            self.session.frame(event.timestamp_ns, event.timestamp_ns)?;
            self.session
                .pen(event)
                .map_err(|_| "Pen queue remained full")?;
        }
        self.dirty = true;
        Ok(())
    }
    /// Records: x/y, pressure, tilt x/y, twist, distance, monotonic ns, phase.
    /// Phase 0 hover, 1 down, 2 move, 3 up, 4 cancel. Tool 0 pen, 1 mouse,
    /// 2 eraser, 3 touch. Pointer routing is decided by the shared core.
    pub fn pointer(
        &mut self,
        id: u64,
        tool: u8,
        button: u8,
        records: &[f64],
    ) -> Result<(), String> {
        if records.is_empty()
            || !records.len().is_multiple_of(9)
            || !records.iter().all(|n| n.is_finite())
            || records
                .chunks_exact(9)
                .any(|r| r[7] < 0.0 || r[8] < 0.0 || r[8] > 4.0 || r[8].fract() != 0.0)
        {
            return Err("Invalid Android pointer batch".into());
        }
        for sample in records.chunks_exact(9) {
            let phase = match sample[8] as u8 {
                0 => PenPhase::Hover,
                1 => PenPhase::Down,
                2 => PenPhase::Move,
                3 => PenPhase::Up,
                _ => PenPhase::Cancel,
            };
            let contact = match phase {
                PenPhase::Hover => None,
                PenPhase::Down => Some(ContactPhase::Down),
                PenPhase::Move => Some(ContactPhase::Move),
                PenPhase::Up => Some(ContactPhase::Up),
                PenPhase::Cancel => Some(ContactPhase::Cancel),
            };
            let position = [sample[0] as f32, sample[1] as f32];
            let paint = if let Some(phase) = contact {
                self.input(UiInput::Pointer {
                    id,
                    phase,
                    kind: match tool {
                        3 => PointerKind::Touch,
                        1 => PointerKind::Mouse,
                        _ => PointerKind::Pen,
                    },
                    button: match button {
                        0 => PointerButton::Primary,
                        1 => PointerButton::Pan,
                        _ => PointerButton::Other,
                    },
                    position,
                })?
                .paint
            } else {
                false
            };
            let event = PenEvent {
                device_id: id,
                sequence: self.sequence + 1,
                timestamp_ns: sample[7] as u64,
                view_revision: self.session.state().camera.revision,
                surface_position: Point {
                    x: position[0],
                    y: position[1],
                },
                pressure: sample[2] as f32,
                tilt_radians: [sample[3] as f32, sample[4] as f32],
                twist_radians: sample[5] as f32,
                distance: sample[6] as f32,
                phase,
                tool: match tool {
                    1 => ToolKind::Mouse,
                    2 => ToolKind::Eraser,
                    _ => ToolKind::Pen,
                },
                flags: SampleFlags::PRIMARY,
            };
            if tool != 3 {
                self.session.cursor_input(if phase == PenPhase::Cancel {
                    None
                } else {
                    Some(event)
                });
                self.dirty = true;
            }
            if paint && self.session.engine().backend().0.is_some() {
                self.sequence += 1;
                self.enqueue(event)?;
                self.last_pen = if matches!(phase, PenPhase::Up | PenPhase::Cancel) {
                    None
                } else {
                    Some(event)
                };
            }
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Value {
        let layout = self.session.layout(self.logical);
        let panels: Vec<_> = layout
            .groups
            .iter()
            .flat_map(|g| &g.panels)
            .filter_map(|&p| self.session.panel_view(p).ok())
            .collect();
        json!({"state": self.session.state(), "layout": layout, "panels": panels,
            "preferences": self.session.preferences(), "picker": self.session.tool_picker(),
            "chrome_hidden": self.chrome_hidden, "gpu_ready": self.session.engine().backend().0.is_some(),
            "error": self.error})
    }
    pub fn query(&self, query: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Query {
            Catalog,
            Context {
                target: layer_ui::ContextTarget,
            },
            Drop {
                position: [f32; 2],
                tabs: Vec<layer_ui::TabHit>,
                item: layer_ui::DockItem,
                #[serde(default)]
                expansion: Option<layer_ui::PanelExpansion>,
            },
            Expansion {
                panel: layer_ui::Panel,
                heights: [f32; 2],
                progress: f32,
            },
        }
        let result = match serde_json::from_value(query).map_err(|e| e.to_string())? {
            Query::Catalog => json!(layer_ui::ui_catalog()),
            Query::Context { target } => json!(self.session.context_menu(target)?),
            Query::Drop {
                position,
                tabs,
                item,
                expansion,
            } => json!(
                self.session
                    .drop_hint(self.logical, position, &tabs, item, expansion)
            ),
            Query::Expansion {
                panel,
                heights,
                progress,
            } => json!(self.session.state().workspace.layout.expanded_panel(
                self.logical,
                panel,
                heights,
                progress
            )),
        };
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_is_available_without_a_gpu() {
        let mut app = App::new().unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        app.dispatch(UiAction::OpenSettings {
            page: layer_ui::SettingsPage::Input,
        })
        .unwrap();
        let snapshot = app.snapshot();
        assert_eq!(snapshot["state"]["platform"], "android");
        assert_eq!(snapshot["gpu_ready"], false);
        assert_eq!(snapshot["preferences"]["page"], "input");
        assert!(!snapshot["layout"]["groups"].as_array().unwrap().is_empty());
        assert_eq!(
            app.query(json!({"type":"catalog"})).unwrap()["app_name"],
            "Capy Canvas"
        );
    }
    #[test]
    fn malformed_input_is_rejected_before_changing_state() {
        let mut app = App::new().unwrap();
        for sample in [
            vec![],
            vec![0.0; 8],
            vec![f64::NAN; 9],
            vec![0., 0., 1., 0., 0., 0., 0., -1., 1.],
        ] {
            assert!(app.pointer(1, 0, 0, &sample).is_err());
        }
        assert_eq!(app.sequence, 0);
        assert!(app.resize(0, 100, 1.0).is_err());
        assert!(app.resize(100, 100, 0.0).is_err());
    }
    #[test]
    fn touch_uses_shared_navigation_not_paint() {
        let mut app = App::new().unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        let before = app.session.state().camera.revision;
        app.pointer(1, 3, 0, &[100., 100., 1., 0., 0., 0., 0., 1_000_000., 1.])
            .unwrap();
        app.pointer(2, 3, 0, &[200., 100., 1., 0., 0., 0., 0., 1_000_000., 1.])
            .unwrap();
        app.pointer(1, 3, 0, &[120., 120., 1., 0., 0., 0., 0., 2_000_000., 2.])
            .unwrap();
        app.pointer(1, 3, 0, &[120., 120., 1., 0., 0., 0., 0., 3_000_000., 3.])
            .unwrap();
        assert_eq!(app.sequence, 0);
        assert!(app.session.state().camera.revision > before);
    }
}
