//! Per-frame scene inputs shared by the native presenters. Hosts own surface
//! acquisition, pacing, presenter construction and presentation.
use crate::NativeHost;
use layer_render_wgpu::{
    BackdropBlurStyle, BackdropRegion, OverviewPlacement, ViewportPresenter,
    local_tone::GpuToneGuide,
};
use layer_ui::{CanvasCursor, DrawerConnection};
use serde::Deserialize;
use std::sync::Arc;

const MAX_SLOTS: usize = 32;
const MAX_GLASS: usize = 256;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigatorSlot {
    pub bounds: [f32; 4],
    pub clip: [f32; 4],
    pub order: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SlotUnits {
    #[default]
    Logical,
    Physical,
}

/// Navigator image slots in native layout coordinates. Empty slots are dropped.
#[derive(Default)]
pub struct Navigators {
    slots: Vec<NavigatorSlot>,
    units: SlotUnits,
}
impl Navigators {
    pub fn new(units: SlotUnits) -> Self {
        Self {
            slots: Vec::new(),
            units,
        }
    }

    pub fn set(&mut self, mut slots: Vec<NavigatorSlot>) -> Result<bool, String> {
        if slots.len() > MAX_SLOTS
            || slots
                .iter()
                .any(|s| !s.bounds.iter().chain(&s.clip).all(|v| v.is_finite()))
        {
            return Err("Invalid Navigator geometry".into());
        }
        slots.retain(|s| s.bounds[2] > 0. && s.bounds[3] > 0. && s.clip[2] > 0. && s.clip[3] > 0.);
        slots.sort_by_key(|s| s.order);
        let changed = self.slots != slots;
        self.slots = slots;
        Ok(changed)
    }

    pub fn count(&self) -> usize {
        self.slots.len()
    }

    /// Surface-pixel placements that sample the live composition and camera.
    pub fn placements(&self, host: &NativeHost, scale: f32) -> Vec<OverviewPlacement> {
        let state = host.session.state();
        let document = host.session.engine().document();
        let fg = state.palette.text.linear();
        let bg = state.palette.panel.linear();
        let units = match self.units {
            SlotUnits::Logical => 1.,
            SlotUnits::Physical => scale,
        };
        self.slots
            .iter()
            .filter_map(|slot| {
                let [x, y, w, h] = slot.bounds.map(|v| v / units);
                let g = layer_ui::NavigatorGeometry::new(
                    &state.camera,
                    [document.width, document.height],
                    [w, h],
                )?;
                Some(OverviewPlacement {
                    bounds: [
                        (x + g.image.x) * scale,
                        (y + g.image.y) * scale,
                        g.image.width * scale,
                        g.image.height * scale,
                    ],
                    clip: Some(slot.clip.map(|v| v / units * scale)),
                    work_area: g.work_area.map(|[a, b]| [(x + a) * scale, (y + b) * scale]),
                    outline_linear: [fg[0], fg[1], fg[2]],
                    background_linear: [bg[0], bg[1], bg[2]],
                    scale,
                    opacity: 1.,
                })
            })
            .collect()
    }
}

/// Glass regions are `[x, y, w, h, tl, tr, br, bl]` in logical points, with an
/// optional ninth value of 1 for a squircle outline.
#[derive(Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlassLayout {
    regions: Vec<Vec<f32>>,
    connections: Vec<DrawerConnection>,
}

#[derive(Default)]
pub struct Glass {
    layout: GlassLayout,
    scaled: Vec<BackdropRegion>,
    scale: f32,
}
impl Glass {
    pub fn set(&mut self, layout: GlassLayout) -> Result<bool, String> {
        let regions = layout.regions.iter().all(|r| {
            matches!(r.len(), 8 | 9) && r.iter().all(|v| v.is_finite()) && r[2] >= 0. && r[3] >= 0.
        });
        let connections = layout.connections.iter().all(|c| {
            [
                c.bounds.x,
                c.bounds.y,
                c.bounds.width,
                c.bounds.height,
                c.length,
                c.depth,
            ]
            .iter()
            .chain(&c.transform)
            .chain(&c.radii)
            .all(|v| v.is_finite())
        });
        if layout.regions.len() + layout.connections.len() > MAX_GLASS || !regions || !connections {
            return Err("Invalid glass geometry".into());
        }
        let changed = self.layout != layout;
        if changed {
            self.layout = layout;
            self.scale = 0.;
        }
        Ok(changed)
    }

    pub fn regions(&mut self, scale: f32) -> &[BackdropRegion] {
        if self.scale != scale {
            self.scale = scale;
            let boxes = self.layout.regions.iter().map(|b| {
                BackdropRegion::rounded(
                    [b[0], b[1], b[2], b[3]].map(|v| v * scale),
                    [b[4], b[5], b[6], b[7]].map(|v| v * scale),
                    if b.get(8) == Some(&1.) {
                        BackdropRegion::SQUIRCLE
                    } else {
                        BackdropRegion::CIRCULAR
                    },
                )
            });
            let connections =
                self.layout
                    .connections
                    .iter()
                    .flat_map(|c| c.glass())
                    .map(|(bounds, radii)| BackdropRegion {
                        bounds: bounds.map(|v| v * scale),
                        radii: radii.map(|v| v * scale),
                        shape: BackdropRegion::SQUIRCLE,
                    });
            self.scaled = boxes.chain(connections).collect();
        }
        &self.scaled
    }

    pub fn count(&self) -> usize {
        self.layout.regions.len() + self.layout.connections.len()
    }
}

pub struct SceneDisplay {
    /// Surface pixels per logical point.
    pub scale: f32,
    /// Display headroom for the SDR-relative HDR view.
    pub headroom: f32,
    pub blank_presented: bool,
    /// The platform compositor tone maps the HDR surface itself.
    pub compositor_hdr: bool,
}

impl NativeHost {
    /// Applies this frame's cursor, color picker, proof, HDR view, tone guide,
    /// navigators and glass. Optional overview and backdrop pipelines wait for
    /// the first paper frame and a ready canvas.
    pub fn compose_scene(
        &mut self,
        presenter: &mut ViewportPresenter,
        cursor: &mut CanvasCursor,
        navigators: &Navigators,
        glass: &[BackdropRegion],
        tone: Option<Arc<GpuToneGuide>>,
        display: SceneDisplay,
    ) -> Result<(), String> {
        let error = |e: layer_render_wgpu::GpuRasterError| e.to_string();
        self.session.update_canvas_cursor(cursor);
        self.session.append_layer_overlay(&mut cursor.segments);
        let proof = self.proof.lut(&self.session);
        let staged = display.blank_presented && self.startup.canvas_ready;
        let overviews = if staged {
            navigators.placements(self, display.scale)
        } else {
            Vec::new()
        };
        let session = &self.session;
        let state = session.state();
        let style = state.palette.glass;
        let backdrop = if staged && style.transparency.enabled() {
            glass
        } else {
            &[]
        };
        let gpu = session
            .engine()
            .backend()
            .0
            .as_deref()
            .ok_or("Missing native renderer")?;
        let rendition = session
            .engine()
            .document()
            .color
            .depth
            .is_float()
            .then(|| session.effective_sdr_rendition());
        if display.compositor_hdr {
            let rendition = rendition.ok_or("HDR output requires HDR artwork")?;
            presenter
                .set_compositor_hdr_view(gpu, rendition)
                .map_err(error)?;
        } else {
            let headroom = if session.hdr_presentation_allowed() {
                display.headroom.max(1.)
            } else {
                1.
            };
            presenter
                .set_hdr_view(gpu, rendition, headroom)
                .map_err(error)?;
        }
        presenter
            .set_gpu_local_tone_guide(gpu, tone)
            .map_err(error)?;
        presenter
            .set_proof(gpu, proof, state.soft_proof, state.gamut_warning)
            .map_err(error)?;
        presenter.set_cursor(gpu.device(), &cursor.segments, display.scale);
        presenter.set_color_picker(gpu, session.color_picker_overlay());
        presenter.set_overviews(gpu, &overviews);
        presenter.set_backdrop(
            gpu,
            backdrop,
            BackdropBlurStyle {
                levels: style.blur.levels,
                offset: style.blur.offset,
            },
            session.engine().has_active_stroke(),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass(value: serde_json::Value) -> Result<GlassLayout, serde_json::Error> {
        serde_json::from_value(value)
    }

    #[test]
    fn glass_regions_scale_once_and_reject_invalid_geometry() {
        let mut layer = Glass::default();
        let layout =
            serde_json::json!({"regions": [[10, 20, 100, 50, 8, 8, 8, 8]], "connections": []});
        assert!(layer.set(glass(layout.clone()).unwrap()).unwrap());
        assert!(!layer.set(glass(layout).unwrap()).unwrap());
        let region = layer.regions(1.5)[0];
        assert_eq!(region.bounds, [15., 30., 150., 75.]);
        assert_eq!(region.radii, [12.; 4]);
        assert_eq!(region.shape, BackdropRegion::CIRCULAR);
        assert_eq!(layer.count(), 1);
        for invalid in [
            serde_json::json!({"regions": [[0, 0, -1, 10, 0, 0, 0, 0]], "connections": []}),
            serde_json::json!({"regions": [[0, 0, 10, 10, 0, 0, 0]], "connections": []}),
            serde_json::json!({"regions": vec![[0, 0, 1, 1, 0, 0, 0, 0]; 257], "connections": []}),
        ] {
            assert!(layer.set(glass(invalid).unwrap()).is_err());
            assert_eq!(layer.count(), 1, "Failures keep the previous layout");
        }
        assert!(glass(serde_json::json!({"regions": [], "connections": [], "extra": 1})).is_err());
        let squircle =
            serde_json::json!({"regions": [[0, 0, 36, 36, 18, 18, 18, 18, 1]], "connections": []});
        assert!(layer.set(glass(squircle).unwrap()).unwrap());
        assert_eq!(layer.regions(1.)[0].shape, BackdropRegion::SQUIRCLE);
        let connection = DrawerConnection {
            bounds: layer_ui::Bounds {
                x: 100.,
                y: 200.,
                width: 60.,
                height: 12.,
            },
            transform: [1., 0., 0., 1., 6., 0.],
            length: 48.,
            depth: 12.,
            radii: [6., 6.],
            square_corners: [false; 4],
        };
        let connected = serde_json::json!({"regions": [], "connections": [connection]});
        assert!(layer.set(glass(connected).unwrap()).unwrap());
        assert_eq!(
            layer.regions(2.).len(),
            3,
            "A connector adds its body and both feet"
        );
        assert!(
            layer
                .set(glass(serde_json::json!({"regions": [], "connections": []})).unwrap())
                .unwrap()
        );
        assert!(layer.regions(1.5).is_empty());
    }

    #[test]
    fn navigator_slots_are_bounded_ordered_and_scaled() {
        let host = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        let slot = |x: f32, order: i32| NavigatorSlot {
            bounds: [x, 20., 264., 200.],
            clip: [x, 30., 200., 170.],
            order,
        };
        let mut navigators = Navigators::default();
        assert!(navigators.set(vec![slot(30., 9), slot(10., 3)]).unwrap());
        assert!(!navigators.set(vec![slot(10., 3), slot(30., 9)]).unwrap());
        let logical = navigators.placements(&host, 2.);
        assert_eq!(logical.len(), 2);
        assert!(logical[0].bounds[0] < logical[1].bounds[0]);
        assert_eq!(logical[0].clip, Some([20., 60., 400., 340.]));
        assert_eq!(logical[0].scale, 2.);
        let mut empty = slot(0., 0);
        empty.bounds[2] = 0.;
        assert!(navigators.set(vec![empty]).unwrap());
        assert_eq!(navigators.count(), 0);
        let mut invalid = slot(0., 0);
        invalid.clip[0] = f32::NAN;
        assert!(navigators.set(vec![invalid]).is_err());
        assert!(navigators.set(vec![slot(0., 0); 33]).is_err());
        navigators.set(vec![slot(10., 3)]).unwrap();
        let mut physical = Navigators::new(SlotUnits::Physical);
        physical
            .set(vec![NavigatorSlot {
                bounds: [20., 40., 528., 400.],
                clip: [20., 60., 400., 340.],
                order: 3,
            }])
            .unwrap();
        let from_physical = physical.placements(&host, 2.);
        let from_logical = navigators.placements(&host, 2.);
        assert_eq!(from_physical[0].bounds, from_logical[0].bounds);
        assert_eq!(from_physical[0].clip, from_logical[0].clip);
        let mut host = host;
        host.dispatch(layer_ui::UiAction::Invoke {
            command: layer_ui::CommandId::RotateRight,
        })
        .unwrap();
        let rotated = navigators.placements(&host, 2.)[0];
        assert_eq!(rotated.bounds, from_logical[0].bounds);
        assert_ne!(rotated.work_area, from_logical[0].work_area);
        let scaled = navigators.placements(&host, 1.)[0];
        assert_eq!(scaled.bounds, rotated.bounds.map(|v| v / 2.));
    }
}
