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
    #[serde(default = "opaque")]
    pub opacity: f32,
}

fn opaque() -> f32 { 1. }

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
                .any(|s| !s.bounds.iter().chain(&s.clip).all(|v| v.is_finite())
                    || !s.opacity.is_finite() || !(0. ..=1.).contains(&s.opacity))
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
                    [document.composition().size[0], document.composition().size[1]],
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
                    scale,
                    opacity: slot.opacity,
                })
            })
            .collect()
    }
}

/// Glass regions are `[x, y, w, h, tl, tr, br, bl]` in logical points, with an
/// optional ninth value of 1 for a squircle outline and tenth value for strength.
#[derive(Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlassLayout {
    regions: Vec<Vec<f32>>,
    connections: Vec<GlassConnection>,
    #[serde(default)]
    arcs: Vec<[f32; 7]>,
    #[serde(default)]
    color_bubbles: Vec<[f32; 7]>,
    #[serde(default)]
    panels: Vec<GlassPanel>,
    #[serde(default)]
    panel_revision: u64,
}

#[derive(PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct GlassPanelGroup {
    group: u32,
    tab_bar: [f32; 4],
    active_tab: [f32; 4],
    tab_clip: [f32; 4],
    color: [f32; 4],
    strip_color: [f32; 4],
}

#[derive(PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct GlassPanel {
    offset: [f32; 2],
    sidebar: [f32; 4],
    radii: [f32; 4],
    color: [f32; 4],
    bridges: Vec<layer_ui::PanelBridge>,
    groups: Vec<GlassPanelGroup>,
}

impl GlassPanel {
    fn surface(&self) -> Option<layer_render_wgpu::PanelSurface> {
        let bridge=self.bridges.first()?;
        let rect=|mut b:[f32;4]|{b[0]+=self.offset[0];b[1]+=self.offset[1];b};
        Some(layer_render_wgpu::PanelSurface {
            sidebar:rect(self.sidebar),sidebar_radii:self.radii,color:self.color,scale:1.,plane:bridge.plane+self.offset[0],
            bridge_width:bridge.width(),direction:if bridge.direction==layer_ui::Edge::Right {1.} else {-1.},
            anchors:self.bridges.iter().map(|b|rect([b.anchor.x,b.anchor.y,b.anchor.width,b.anchor.height])).collect(),
            groups:bridge.receivers.iter().filter_map(|receiver| {
                let native=self.groups.iter().find(|g|g.group==receiver.group)?;
                let b=receiver.bounds;
                Some(layer_render_wgpu::PanelSurfaceGroup {
                    bounds:rect([b.x,b.y,b.width,b.height]),radii:receiver.radii,tab_bar:rect(native.tab_bar),
                    active_tab:rect(native.active_tab),tab_clip:rect(native.tab_clip),color:native.color,strip_color:native.strip_color,
                })
            }).collect(),
        })
    }
    fn valid(&self) -> bool {
        let rect=|b:[f32;4]|b.into_iter().all(f32::is_finite) && b[2]>=0. && b[3]>=0.;
        let radii=|r:[f32;4]|r.into_iter().all(|v|v.is_finite() && v>=0.);
        let color=|c:[f32;4]|c.into_iter().all(|v|v.is_finite() && (0.0..=1.).contains(&v));
        let Some(first)=self.bridges.first() else {return false;};
        let mut groups=std::collections::HashSet::new();
        self.offset.into_iter().all(f32::is_finite) && rect(self.sidebar) && radii(self.radii) && color(self.color)
            && self.groups.iter().all(|g|groups.insert(g.group) && rect(g.tab_bar) && rect(g.active_tab) && rect(g.tab_clip) && color(g.color) && color(g.strip_color))
            && !first.receivers.is_empty() && first.receivers.len()==self.groups.len()
            && {let mut receivers=std::collections::HashSet::new();first.receivers.iter().all(|r|receivers.insert(r.group) && groups.contains(&r.group)
                && rect([r.bounds.x,r.bounds.y,r.bounds.width,r.bounds.height]) && r.bounds.width>0. && r.bounds.height>0. && radii(r.radii))}
            && self.bridges.iter().all(|b| b.plane.is_finite() && matches!(b.direction,layer_ui::Edge::Left|layer_ui::Edge::Right)
                && b.direction==first.direction && b.plane==first.plane && b.receivers==first.receivers
                && rect([b.anchor.x,b.anchor.y,b.anchor.width,b.anchor.height]) && b.anchor.width>0. && b.anchor.height>0. && b.width()>0.)
    }
}

#[derive(PartialEq, Deserialize)]
struct GlassConnection {
    #[serde(flatten)]
    connection: DrawerConnection,
    #[serde(default = "opaque")]
    strength: f32,
}

#[derive(Default)]
pub struct Glass {
    layout: GlassLayout,
    scaled: Vec<BackdropRegion>,
    scale: f32,
    panels: Vec<layer_render_wgpu::PanelSurface>,
}
impl Glass {
    pub fn set(&mut self, layout: GlassLayout) -> Result<bool, String> {
        let regions = layout.regions.iter().all(|r| {
            matches!(r.len(), 8..=10) && r.iter().all(|v| v.is_finite()) && r[2] >= 0. && r[3] >= 0.
        });
        let arcs = layout.arcs.iter().all(|a| a.iter().all(|v| v.is_finite()) && a[2] >= 0. && a[3] >= 0.);
        let color_bubbles = layout.color_bubbles.iter().all(|c| c.iter().all(|v| v.is_finite())
            && c[2..=4].iter().all(|v| *v >= 0.) && (0.0..=std::f32::consts::FRAC_PI_2).contains(&c[5]));
        let connections = layout.connections.iter().all(|entry| {
            let c = &entry.connection;
            entry.strength.is_finite() && [
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
        let panels=layout.panels.iter().all(GlassPanel::valid);
        let panel_slots:usize=layout.panels.iter().map(|p|1+p.groups.len()+p.bridges.len()).sum();
        if layout.regions.len() + layout.connections.len() + layout.arcs.len() + layout.color_bubbles.len() + panel_slots > MAX_GLASS || !regions || !connections || !arcs || !color_bubbles || !panels {
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
                ).with_strength(b.get(9).copied().unwrap_or(1.))
            });
            let connections =
                self.layout
                    .connections
                    .iter()
                    .flat_map(|c| c.connection.glass().into_iter().map(move |(bounds, radii)| BackdropRegion {
                        bounds: bounds.map(|v| v * scale),
                        radii: radii.map(|v| v * scale),
                        shape: BackdropRegion::SQUIRCLE,
                    }.with_strength(c.strength)));
            let arcs = self.layout.arcs.iter().map(|a| BackdropRegion::arc(
                [a[0] * scale, a[1] * scale], a[2] * scale, a[3] * scale, a[4].to_radians(), a[5].to_radians(),
            ).with_strength(a[6]));
            let color_bubbles = self.layout.color_bubbles.iter().map(|c| BackdropRegion::color_bubble(
                [c[0] * scale, c[1] * scale], c[2] * scale, c[3] * scale, c[4] * scale, c[5],
            ).with_strength(c[6]));
            self.panels=self.layout.panels.iter().filter_map(GlassPanel::surface).map(|p|p.scaled(scale)).collect();
            let panels=self.panels.iter().map(|p|BackdropRegion::rounded(p.bounds(),[0.;4],BackdropRegion::SQUIRCLE).with_strength(0.));
            self.scaled = boxes.chain(connections).chain(arcs).chain(color_bubbles).chain(panels).collect();
        }
        &self.scaled
    }

    pub fn count(&self) -> usize {
        self.layout.regions.len() + self.layout.connections.len() + self.layout.arcs.len() + self.layout.color_bubbles.len() + self.layout.panels.len()
    }
    pub fn panels(&mut self,scale:f32) -> &[layer_render_wgpu::PanelSurface] {self.regions(scale);&self.panels}
    pub fn panel_revision(&self) -> u64 {self.layout.panel_revision}
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
    pub fn poll_canvas_presentation(&mut self, presenter: &mut ViewportPresenter) -> Result<(), String> {
        if self.session.rendering_suspended() { return Ok(()); }
        let Some(renderer)=self.session.renderer_mut().0.as_mut() else { return Ok(()); };
        renderer.poll_startup().map_err(|e|e.to_string())?;
        presenter.prepare_paper_lighting(renderer);
        let progress=renderer.poll_startup().map_err(|e|e.to_string())?;
        self.dirty|=presenter.prepare_paper_lighting(renderer)!=presenter.paper_lighting_drawn();
        if self.startup!=progress {
            self.startup=progress;
            self.invalidate_snapshot();
        }
        Ok(())
    }

    pub fn set_glass(&mut self, glass: &mut Glass, layout: GlassLayout) -> Result<(), String> {
        if glass.set(layout)? {
            self.dirty = true;
        }
        Ok(())
    }

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
            .composition().color
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
            session.hold_canvas_backdrop(),
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
    fn joined_panel_glass_keeps_receiver_corners_and_scales_allocated_geometry_once() {
        let packet=serde_json::json!({"regions":[],"connections":[],"panel_revision":7,"panels":[{
            "offset":[-2.,3.],"sidebar":[10.,20.,36.,200.],"radii":[18.,0.,6.,12.],"color":[0.2,0.3,0.4,0.8],
            "bridges":[{"anchor":{"x":10.,"y":20.,"width":36.,"height":36.},"plane":52.,"direction":"right",
                "receivers":[{"group":5,"bounds":{"x":52.,"y":20.,"width":200.,"height":160.},"radii":[18.,12.,6.,0.]}]}],
            "groups":[{"group":5,"tab_bar":[52.,20.,200.,36.],"active_tab":[58.,20.,80.,36.],
                "tab_clip":[58.,20.,188.,36.],"color":[0.2,0.3,0.4,0.8],"strip_color":[0.1,0.2,0.3,0.8]}]
        }]});
        let mut layer=Glass::default();assert!(layer.set(glass(packet.clone()).unwrap()).unwrap());
        assert_eq!(layer.panel_revision(),7);
        let expected=layer.panels(1.5).to_vec();let surface=&expected[0];
        assert_eq!(surface.sidebar,[12.,34.5,54.,300.]);
        assert_eq!(surface.sidebar_radii,[27.,0.,9.,18.]);
        assert_eq!(surface.plane,75.);assert_eq!(surface.bridge_width,9.);assert_eq!(surface.direction,1.);
        assert_eq!(surface.anchors,vec![[12.,34.5,54.,54.]]);
        assert_eq!(surface.groups[0].bounds,[75.,34.5,300.,240.]);
        assert_eq!(surface.groups[0].radii,[27.,18.,9.,0.]);
        assert_eq!(surface.groups[0].tab_bar,[75.,34.5,300.,54.]);
        assert_eq!(surface.groups[0].active_tab,[84.,34.5,120.,54.]);
        assert_eq!(surface.groups[0].tab_clip,[84.,34.5,282.,54.]);
        assert_eq!(surface.color,[0.2,0.3,0.4,0.8]);
        assert_eq!(layer.panels(1.5),expected);
        assert!(!layer.set(glass(packet.clone()).unwrap()).unwrap());
        for case in 0..6 {
            let mut invalid=packet.clone();
            match case {
                0=>invalid["panels"][0]["groups"][0]["group"]=999.into(),
                1=>invalid["panels"][0]["bridges"][0]["receivers"][0]["radii"][2]=(-1).into(),
                2=>invalid["panels"][0]["groups"][0]["tab_clip"][2]=(-1).into(),
                3=>invalid["panels"][0]["bridges"][0]["direction"]="top".into(),
                4=>invalid["panels"][0]["groups"]=serde_json::json!([invalid["panels"][0]["groups"][0].clone(),invalid["panels"][0]["groups"][0].clone()]),
                _=>invalid["panels"]=serde_json::Value::Array(vec![packet["panels"][0].clone();MAX_GLASS]),
            }
            assert!(layer.set(glass(invalid).unwrap()).is_err(),"case {case}");
            assert_eq!(layer.panels(1.5),expected,"invalid panel frame replaced live geometry");
            assert_eq!(layer.panel_revision(),7);
        }
        layer.set(glass(serde_json::json!({"regions":[],"connections":[],"panel_revision":8})).unwrap()).unwrap();
        assert!(layer.panels(1.5).is_empty());assert_eq!(layer.panel_revision(),8);
    }

    #[test]
    fn color_bubble_glass_scales_geometry_and_caches_unchanged_packets() {
        let packet = |strength| glass(serde_json::json!({"regions":[],"connections":[],
            "color_bubbles":[[30,40,20,26,4,0.75,strength]]})).unwrap();
        let mut layer = Glass::default();
        assert!(layer.set(packet(0.375)).unwrap());
        assert_eq!(layer.count(), 1);
        for scale in [2., 1.5, 2.] {
            let expected = BackdropRegion::color_bubble([30.*scale,40.*scale],20.*scale,26.*scale,4.*scale,0.75).with_strength(0.375);
            assert_eq!(layer.regions(scale), [expected]);
            assert_eq!(layer.regions(scale)[0].radii, [20.*scale,26.*scale,4.*scale,0.75]);
            let cached = layer.regions(scale).as_ptr();
            assert!(!layer.set(packet(0.375)).unwrap());
            assert_eq!(layer.regions(scale).as_ptr(), cached);
        }
        let before = layer.regions(2.)[0];
        assert!(layer.set(packet(0.125)).unwrap());
        let after = layer.regions(2.)[0];
        assert_eq!(before.bounds, after.bounds);
        assert_eq!(before.radii, after.radii);
        assert_eq!(after.shape[1], 0.125);
        assert_eq!(before.shape[2..], after.shape[2..]);
        assert!(layer.set(packet(2.)).unwrap());
        assert_eq!(layer.regions(2.)[0].shape[1], 1.);
    }

    #[test]
    fn invalid_color_bubble_glass_preserves_the_cached_layout_and_shares_the_limit() {
        let valid = || glass(serde_json::json!({"regions":[[1,2,30,40,3,3,3,3]],"connections":[],
            "arcs":[[20,30,12,4,0,2,0.75]],"color_bubbles":[[30,40,20,26,4,0.75,0.5]]})).unwrap();
        let mut layer = Glass::default();
        layer.set(valid()).unwrap();
        let before = layer.regions(1.5).to_vec();
        for index in 0..7 {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut invalid = valid(); invalid.color_bubbles[0][index] = value;
                assert!(layer.set(invalid).is_err());
                assert_eq!(layer.regions(1.5), before);
            }
        }
        for (index, value) in [(2,-1.),(3,-1.),(4,-1.),(5,-0.01),(5,std::f32::consts::FRAC_PI_2+0.01)] {
            let mut invalid = valid(); invalid.color_bubbles[0][index] = value;
            assert!(layer.set(invalid).is_err());
            assert_eq!(layer.regions(1.5), before);
        }
        let mut maximum = valid(); maximum.color_bubbles = vec![[0.;7]; MAX_GLASS-2];
        assert!(layer.set(maximum).unwrap());
        assert_eq!(layer.count(), MAX_GLASS);
        let cached = layer.regions(1.5).to_vec();
        let mut oversized = valid(); oversized.color_bubbles = vec![[0.;7]; MAX_GLASS-1];
        assert!(layer.set(oversized).is_err());
        assert_eq!(layer.regions(1.5), cached);
        for entry in [serde_json::json!([0,0,1,2,3,0]), serde_json::json!([0,0,1,2,3,0,1,1])] {
            assert!(glass(serde_json::json!({"regions":[],"connections":[],"color_bubbles":[entry]})).is_err());
        }
        assert!(glass(serde_json::json!({"regions":[],"connections":[]})).unwrap().color_bubbles.is_empty());
    }

    #[test]
    fn glass_arcs_scale_geometry_once_without_scaling_angles_or_fade() {
        let mut layer = Glass::default();
        let layout = |strength| serde_json::json!({"regions":[[10,20,100,50,4,5,6,7,1,strength]],"connections":[],"arcs":[[30,40,20,6,15.,-90.,strength]]});
        assert!(layer.set(glass(layout(0.375)).unwrap()).unwrap());
        for scale in [2., 1.5, 2.] {
            let regions = layer.regions(scale);
            assert_eq!(regions[0].bounds, [10.,20.,100.,50.].map(|v| v*scale));
            assert_eq!(regions[0].radii, [4.,5.,6.,7.].map(|v| v*scale));
            assert_eq!(regions[0].shape, [4.,0.375,0.,0.]);
            let arc = BackdropRegion::arc([30.*scale,40.*scale],20.*scale,6.*scale,15f32.to_radians(),(-90f32).to_radians()).with_strength(0.375);
            assert_eq!(regions[1], arc);
            assert_eq!(regions[1].radii, [30.*scale,40.*scale,20.*scale,3.*scale]);
            assert_eq!(regions[1].shape, [0.,0.375,(-75f32).to_radians(),90f32.to_radians()]);
        }
        let before = layer.regions(2.).to_vec();
        assert!(layer.set(glass(layout(0.125)).unwrap()).unwrap());
        assert!(!layer.set(glass(layout(0.125)).unwrap()).unwrap());
        for (old, new) in before.iter().zip(layer.regions(2.)) {
            assert_eq!(old.bounds, new.bounds);
            assert_eq!(old.radii, new.radii);
            assert_eq!(new.shape[1], 0.125);
            assert_eq!(old.shape[2..], new.shape[2..]);
        }
    }

    #[test]
    fn invalid_arc_or_fade_preserves_the_last_valid_glass_layout() {
        let valid = || glass(serde_json::json!({"regions":[[1,2,30,40,3,3,3,3,0,0.5]],"connections":[],"arcs":[[20,30,12,4,0,2,0.75]]})).unwrap();
        let mut layer = Glass::default();
        layer.set(valid()).unwrap();
        let before = layer.regions(1.5).to_vec();
        for index in 0..7 {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut invalid = valid();
                invalid.arcs[0][index] = value;
                assert!(layer.set(invalid).is_err());
                assert_eq!(layer.regions(1.5), before);
            }
        }
        for index in [2,3] {
            let mut invalid = valid(); invalid.arcs[0][index] = -1.;
            assert!(layer.set(invalid).is_err());
            assert_eq!(layer.regions(1.5), before);
        }
        let mut invalid = valid(); invalid.regions[0][9] = f32::NAN;
        assert!(layer.set(invalid).is_err());
        let mut invalid = valid(); invalid.arcs = vec![[0.;7]; MAX_GLASS];
        assert!(layer.set(invalid).is_err());
        assert_eq!(layer.count(), 2);
        assert_eq!(layer.regions(1.5), before);
        let mut maximum = valid(); maximum.arcs = vec![[0.;7]; MAX_GLASS-1];
        assert!(layer.set(maximum).unwrap());
        assert_eq!(layer.count(), MAX_GLASS);
        assert!(glass(serde_json::json!({"regions":[],"connections":[],"arcs":[[0,0,1,1,0,1]]})).is_err());
    }

    #[test]
    fn glass_motion_and_fade_dirty_render_without_republishing_the_model() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let mut layer = Glass::default();
        host.take_update_bytes().unwrap().unwrap();
        let revision = host.session.state().revision;
        let layout = |x, strength| glass(serde_json::json!({
            "regions":[[x,20,100,50,8,8,8,8,1,strength]],"connections":[],
            "arcs":[[x,40,20,6,0.25,1.5,strength]]
        })).unwrap();
        host.set_glass(&mut layer, layout(10., 1.)).unwrap();
        assert!(host.take_update_bytes().unwrap().is_none());
        let mut previous = layer.regions(1.5).to_vec();
        for (x, strength) in [(30.,1.),(30.,0.5),(60.,0.125)] {
            host.dirty = false;
            host.set_glass(&mut layer, layout(x, strength)).unwrap();
            assert!(host.dirty, "Motion and fade must redraw glass");
            assert_eq!(layer.count(), 2);
            assert_ne!(layer.regions(1.5), previous);
            previous = layer.regions(1.5).to_vec();
            assert!(host.take_update_bytes().unwrap().is_none(), "Unchanged metadata must not republish the UI model");
            assert_eq!(host.session.state().revision, revision);
            host.dirty = false;
            host.set_glass(&mut layer, layout(x, strength)).unwrap();
            assert!(!host.dirty, "Identical geometry must remain idle");
        }
        host.set_glass(&mut layer, glass(serde_json::json!({"regions":[],"connections":[]})).unwrap()).unwrap();
        assert!(host.dirty);
        assert_eq!(layer.count(), 0);
        assert!(host.take_update_bytes().unwrap().is_none(), "Glass topology must retain the UI model");
        assert_eq!(host.session.state().revision, revision);
    }

    #[test]
    fn glass_topology_retains_layout_publications_and_document_state() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let mut layer = Glass::default();
        assert!(host.take_layout_update_bytes().unwrap().is_some());
        assert!(host.take_layout_update_bytes().unwrap().is_none());
        let revision = host.session.state().revision;
        let publication = serde_json::to_value(host.session.workspace_update()).unwrap();
        for regions in [
            vec![[10, 20, 100, 50, 8, 8, 8, 8]],
            vec![[10, 20, 100, 50, 8, 8, 8, 8], [50, 80, 200, 40, 8, 8, 8, 8]],
            vec![],
        ] {
            let layout = serde_json::json!({"regions": regions, "connections": []});
            host.dirty = false;
            host.set_glass(&mut layer, glass(layout.clone()).unwrap()).unwrap();
            assert!(host.dirty);
            assert!(host.take_layout_update_bytes().unwrap().is_none());
            assert_eq!(layer.count(), regions.len());
            assert_eq!(serde_json::to_value(host.session.workspace_update()).unwrap(), publication);
            assert_eq!(host.session.state().revision, revision);
            host.dirty = false;
            host.set_glass(&mut layer, glass(layout).unwrap()).unwrap();
            assert!(!host.dirty);
            assert!(host.take_update_bytes().unwrap().is_none());
        }
        let invalid = serde_json::json!({"regions": [[0, 0, -1, 10, 0, 0, 0, 0]], "connections": []});
        assert!(host.set_glass(&mut layer, glass(invalid).unwrap()).is_err());
        assert!(!host.dirty);
        assert_eq!(layer.count(), 0);
        assert!(host.take_update_bytes().unwrap().is_none());
        assert_eq!(host.session.state().revision, revision);
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
        assert!(layer.set(glass(connected.clone()).unwrap()).unwrap());
        assert_eq!(
            layer.regions(2.).len(),
            3,
            "A connector adds its body and both feet"
        );
        let geometry = layer.regions(2.).to_vec();
        assert!(geometry.iter().all(|region| region.shape[1] == 1.));
        for strength in [0.375f32, 0., 2., -1.] {
            let mut packet = connected.clone();
            packet["connections"][0]["strength"] = strength.into();
            layer.set(glass(packet).unwrap()).unwrap();
            for (region, original) in layer.regions(2.).iter().zip(&geometry) {
                assert_eq!(region.bounds, original.bounds);
                assert_eq!(region.radii, original.radii);
                assert_eq!(region.shape[1], strength.clamp(0., 1.));
            }
        }
        let cached = layer.regions(2.).to_vec();
        let mut invalid = glass(connected).unwrap();
        invalid.connections[0].strength = f32::NAN;
        assert!(layer.set(invalid).is_err());
        assert_eq!(layer.regions(2.), cached);
        assert!(
            layer
                .set(glass(serde_json::json!({"regions": [], "connections": []})).unwrap())
                .unwrap()
        );
        assert!(layer.regions(1.5).is_empty());
    }

    #[test]
    fn navigator_opacity_survives_layout_and_rejects_invalid_values() {
        let host = NativeHost::new(layer_ui::Platform::Android).unwrap();
        let mut slot: NavigatorSlot = serde_json::from_value(serde_json::json!({
            "bounds":[0.,0.,200.,100.],"clip":[0.,0.,200.,100.],"order":0,
        })).unwrap();
        assert_eq!(slot.opacity, 1.);
        let mut navigators = Navigators::default();
        for opacity in [0., 0.25, 1.] {
            slot.opacity = opacity;
            navigators.set(vec![slot.clone()]).unwrap();
            assert_eq!(navigators.placements(&host, 2.)[0].opacity, opacity);
        }
        for opacity in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            slot.opacity = opacity;
            assert!(navigators.set(vec![slot.clone()]).is_err());
            assert_eq!(navigators.placements(&host, 2.)[0].opacity, 1.);
        }
    }

    #[test]
    fn navigator_slots_are_bounded_ordered_and_scaled() {
        let host = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        let slot = |x: f32, order: i32| NavigatorSlot {
            bounds: [x, 20., 264., 200.],
            clip: [x, 30., 200., 170.],
            order,
            opacity: 1.,
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
                opacity: 1.,
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
