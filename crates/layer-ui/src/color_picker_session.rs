//! Shared color-picking ownership, reversible previews and tool restoration.
//! Hosts recognize a hold; no clocks, GTK widgets or display captures live here.
use super::*;
use layer_core::Point;
use layer_render::{ColorPickerOverlay, ColorSampleSource};

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn picker_cancel_action(&self, action: &UiAction) -> bool {
        self.eyedropper.picking.previous.is_some()
            && match action {
                UiAction::Invoke { command } => {
                    *command != CommandId::Eyedropper && CommandId::TOOLS.contains(command)
                }
                UiAction::Layer {
                    action: LayerAction::Tool { tool },
                } => !tool.picks_color(),
                UiAction::SelectBrush { .. }
                | UiAction::SelectToolGroup { .. }
                | UiAction::SelectBrushSet { .. }
                | UiAction::CycleTool { .. } => true,
                _ => false,
            }
    }

    pub(super) fn picker_layer_available(&self) -> bool {
        let doc = self.engine.document();
        !doc.active_mask
            && !doc.is_locked(doc.active_layer)
            && doc
                .layer(doc.active_layer)
                .is_some_and(|l| l.kind == LayerKind::Paint && l.source.is_none())
    }

    pub(crate) fn start_picker(&mut self) -> Result<(), String> {
        self.require_idle()?;
        self.cancel_layer_gesture()?;
        // Contacts already resting on the canvas must not resume navigation
        // after picking. This also invalidates their pending native holds.
        self.touch.clear();
        self.eyedropper.cancel();
        self.eyedropper.preview_only = true;
        let previous = self.layer_interaction.tool;
        self.eyedropper.picking.previous = Some(if previous.picks_color() {
            LayerCanvasTool::Paint
        } else {
            previous
        });
        self.eyedropper.picking.original = Some(self.state.display_colors().definition());
        self.eyedropper.picking.finishing = false;
        self.eyedropper.picking.touch = None;
        if self.eyedropper.layer && !self.picker_layer_available() {
            self.eyedropper.layer = false;
        }
        self.layer_interaction.tool = if self.eyedropper.layer {
            LayerCanvasTool::PickLayer
        } else {
            LayerCanvasTool::PickVisible
        };
        self.state.layer_tools.tool = self.layer_interaction.tool;
        self.state.color_picker.preview = None;
        self.refresh_tools();
        self.refresh_commands();
        if let Some(event) = self.cursor.event {
            self.picker_position([event.surface_position.x, event.surface_position.y]);
        }
        Ok(())
    }

    pub(crate) fn cancel_picker(&mut self) -> bool {
        let Some(previous) = self.eyedropper.picking.previous.take() else {
            return false;
        };
        self.eyedropper.cancel();
        self.eyedropper.picking.position = None;
        self.eyedropper.picking.touch = None;
        self.eyedropper.picking.finishing = false;
        self.state.color_picker.preview = None;
        if self
            .state
            .customization
            .drawer
            .as_ref()
            .is_some_and(|d| d.compact)
        {
            self.state.customization.drawer = None;
        }
        self.layer_interaction.tool = previous;
        self.state.layer_tools.tool = previous;
        self.refresh_tools();
        self.refresh_commands();
        true
    }

    pub(super) fn configure_picker(&mut self, action: ColorPickerAction) -> Result<(), String> {
        match action {
            ColorPickerAction::Settings { anchor } => {
                let action = match anchor {
                    DrawerAnchor::Tile { panel, tile } => CustomizationAction::ToggleToolDrawer { anchor: TileAnchor { panel, tile } },
                    DrawerAnchor::Header { id } => CustomizationAction::ToggleHeaderDrawer { id },
                    _ => return Err("Color picker options need a tool anchor".into()),
                };
                if self.eyedropper.picking.previous.is_none() { self.start_picker()?; }
                self.dispatch(UiAction::Customize { action })?;
            }
            ColorPickerAction::Toggle => {
                if !self.cancel_picker() {
                    self.state.color_picker.style = ColorPickerStyle::Glass;
                    self.start_picker()?;
                }
            }
            ColorPickerAction::Style { style } => self.state.color_picker.style = style,
            ColorPickerAction::Source { layer } => {
                if layer && !self.picker_layer_available() {
                    return Err("Select an editable paint layer to sample its color".into());
                }
                self.eyedropper.layer = layer;
                if self.layer_interaction.tool.picks_color() {
                    self.layer_interaction.tool = if layer {
                        LayerCanvasTool::PickLayer
                    } else {
                        LayerCanvasTool::PickVisible
                    };
                    self.state.layer_tools.tool = self.layer_interaction.tool;
                }
                self.resample_picker();
            }
        }
        self.refresh_tools();
        Ok(())
    }

    pub(super) fn resample_picker(&mut self) {
        self.eyedropper.cancel();
        self.state.color_picker.preview = None;
        if let Some(position) = self.eyedropper.picking.position {
            self.picker_position(position);
        }
    }

    fn finish_picker(&mut self, position: [f32; 2]) {
        // Hover may show the most recently completed sample during motion.
        // Acceptance must use this contact's exact point, never an older readback.
        if self.eyedropper.busy() {
            self.eyedropper.cancel();
        }
        self.picker_position(position);
        self.eyedropper.picking.finishing = true;
    }

    pub(super) fn picker_position(&mut self, position: [f32; 2]) {
        if !position.into_iter().all(f32::is_finite) {
            return;
        }
        self.eyedropper.picking.position = Some(position);
        let (_, sample, _) = self.picker_geometry(position);
        let mut point = self.state.camera.input_transform().map(Point {
            x: sample[0],
            y: sample[1],
        });
        let doc = self.engine.document();
        // Even raw-layer sampling is limited to the document, never the surround.
        let inside = point.x >= 0.
            && point.y >= 0.
            && point.x < doc.width as f32
            && point.y < doc.height as f32;
        let (source, extent) = if self.eyedropper.layer && self.picker_layer_available() {
            let Some(inverse) = doc.layer_transform(doc.active_layer).inverse() else {
                return;
            };
            point = inverse.map(point);
            (
                ColorSampleSource::Layer(doc.active_layer),
                doc.target_extent(doc.active_layer),
            )
        } else {
            (ColorSampleSource::Composite, [doc.width, doc.height])
        };
        if inside
            && point.x >= 0.
            && point.y >= 0.
            && point.x < extent[0] as f32
            && point.y < extent[1] as f32
        {
            self.eyedropper
                .queue(source, [point.x.floor() as u32, point.y.floor() as u32]);
        } else {
            self.eyedropper.cancel();
            self.state.color_picker.preview = None;
        }
    }

    pub(super) fn color_picker_input(
        &mut self,
        input: &UiInput,
    ) -> Result<Option<UiChange>, String> {
        let mut changed = regions::COLOR_PREVIEW;
        match *input {
            UiInput::ColorPickerHold {
                id,
                position,
                offset,
            } => {
                if !offset.is_finite()
                    || offset < 0.
                    || !position.into_iter().all(f32::is_finite)
                    || self.eyedropper.picking.previous.is_some()
                    || !self.touch.is_only_contact(id)
                    || self.interaction.pointer.is_some()
                    || self.state.settings_open
                    || self.require_idle().is_err()
                {
                    return Ok(None);
                }
                self.start_picker()?;
                self.eyedropper.picking.touch = Some(id);
                self.eyedropper.picking.touch_offset = offset;
                self.eyedropper.picking.consumed.push((PointerKind::Touch, id));
                self.picker_position(position);
                changed |= regions::BRUSH | regions::COMMANDS | regions::CUSTOMIZATION;
            }
            UiInput::Blur => {
                self.eyedropper.picking.consumed.clear();
                // Continue the ordinary blur route to release navigation and keys.
                return Ok(None);
            }
            UiInput::Key {
                ref key,
                pressed: true,
                editing: false,
                ..
            } if key.eq_ignore_ascii_case("escape")
                && self.eyedropper.picking.previous.is_some() =>
            {
                self.cancel_picker();
                changed |= regions::BRUSH | regions::COMMANDS | regions::CUSTOMIZATION;
            }
            UiInput::Pointer {
                id,
                phase,
                kind,
                button,
                position,
            } => {
                let contact = (kind, id);
                let consumed = self.eyedropper.picking.consumed.contains(&contact);
                if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
                    self.eyedropper.picking.consumed.retain(|&p| p != contact);
                }
                let active = self.eyedropper.picking.previous.is_some();
                if !consumed && (!active || button != PointerButton::Primary) {
                    return Ok(None);
                }
                if phase == ContactPhase::Down && !consumed {
                    self.eyedropper.picking.consumed.push(contact);
                }
                if !active || self.eyedropper.picking.finishing {
                    return Ok(Some(UiChange::default()));
                }
                match kind {
                    PointerKind::Touch => {
                        if self.eyedropper.picking.touch == Some(id) {
                            match phase {
                                ContactPhase::Move => self.picker_position(position),
                                ContactPhase::Up => {
                                    self.finish_picker(position);
                                }
                                ContactPhase::Cancel => {
                                    self.cancel_picker();
                                }
                                _ => (),
                            }
                        } else if phase == ContactPhase::Down {
                            if self.eyedropper.picking.touch.is_some() {
                                if self.picker_layer_available() {
                                    self.configure_picker(ColorPickerAction::Source {
                                        layer: !self.eyedropper.layer,
                                    })?;
                                    changed |= regions::BRUSH;
                                }
                            } else {
                                self.cancel_picker();
                                changed |=
                                    regions::BRUSH | regions::COMMANDS | regions::CUSTOMIZATION;
                            }
                        }
                    }
                    PointerKind::Pen => match phase {
                        ContactPhase::Down => self.picker_position(position),
                        ContactPhase::Move if consumed => self.picker_position(position),
                        ContactPhase::Up if consumed => self.finish_picker(position),
                        ContactPhase::Cancel => {
                            self.cancel_picker();
                        }
                        _ => (),
                    },
                    PointerKind::Mouse => {
                        if phase == ContactPhase::Down {
                            self.finish_picker(position);
                        } else if phase == ContactPhase::Cancel {
                            self.cancel_picker();
                        }
                    }
                }
            }
            _ => return Ok(None),
        }
        if self.eyedropper.picking.previous.is_none() {
            changed |= regions::BRUSH | regions::COMMANDS | regions::CUSTOMIZATION;
        }
        Ok(Some(self.changed(changed, true)))
    }

    /// Touch aims at the visible crosshair above the contact. Keep sampling and
    /// magnification on the same clamped surface point, including at the edges.
    fn picker_geometry(&self, position: [f32; 2]) -> ([f32; 2], [f32; 2], f32) {
        let picking = &self.eyedropper.picking;
        let scale = self
            .logical_viewport
            .map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        let mut center = position;
        if picking.touch.is_some() {
            center[1] -= picking.touch_offset;
        }
        let radius = 46. * scale;
        for (i, value) in center.iter_mut().enumerate() {
            *value = value.clamp(
                radius + 2. * scale,
                (self.state.camera.viewport[i] as f32 - radius - 2. * scale)
                    .max(radius + 2. * scale),
            );
        }
        let sample = if picking.touch.is_some() {
            center
        } else {
            position
        };
        (center, sample, scale)
    }

    /// Physical surface geometry and document-linear colors; display-only GPU
    /// lens samples the existing artwork texture without a CPU image readback.
    pub fn color_picker_overlay(&self) -> Option<ColorPickerOverlay> {
        let picking = &self.eyedropper.picking;
        if picking.previous.is_none() || picking.finishing || self.state.settings_open {
            return None;
        }
        let position = picking.position?;
        let (center, sample, scale) = self.picker_geometry(position);
        let old = picking.original?;
        let new = self.state.color_picker.preview.unwrap_or(old);
        let space = self.engine.document().color.space;
        let classic = self.state.color_picker.style == ColorPickerStyle::Eyedropper
            && picking.touch.is_none();
        Some(ColorPickerOverlay {
            center: if classic { position } else { center },
            sample,
            scale,
            classic,
            layer: self.eyedropper.layer && self.picker_layer_available(),
            original: old.linear_in(space).ok()?,
            candidate: new.linear_in(space).ok()?,
        })
    }
}
