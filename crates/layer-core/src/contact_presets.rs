//! Built-in media using the shared GPU contact model.
use crate::*;
use std::sync::Arc;

pub const CONTACT_BRUSH_PRESETS: [DefaultBrushPreset; 25] = [
    DefaultBrushPreset::Pencil,
    DefaultBrushPreset::PointyPencil,
    DefaultBrushPreset::ShadingPencil,
    DefaultBrushPreset::Charcoal,
    DefaultBrushPreset::Crayon,
    DefaultBrushPreset::GPen,
    DefaultBrushPreset::Monoline,
    DefaultBrushPreset::RoughGPen,
    DefaultBrushPreset::CalligraphyPen,
    DefaultBrushPreset::AntiquePen,
    DefaultBrushPreset::RealisticPen,
    DefaultBrushPreset::WetInk,
    DefaultBrushPreset::BlottyInk,
    DefaultBrushPreset::BrushedInk,
    DefaultBrushPreset::Eraser,
    DefaultBrushPreset::Paintbrush,
    DefaultBrushPreset::BristlePaintbrush,
    DefaultBrushPreset::Airbrush,
    DefaultBrushPreset::Chalk,
    DefaultBrushPreset::Marker,
    DefaultBrushPreset::DualTexture,
    DefaultBrushPreset::TexturedFlat,
    DefaultBrushPreset::DryScumble,
    DefaultBrushPreset::PastelBlock,
    DefaultBrushPreset::TransparentGlaze,
];

pub(crate) fn dry_brush(preset: DefaultBrushPreset) -> BrushSnapshot {
    use DefaultBrushPreset::*;
    let pose = |radii, relief| BrushDryPose { radii, relief };
    let (diameter, flow, paper, loading, corner, poses) = match preset {
        Pencil => (9., 0.70, 1.1, 0.70, 0., [
            pose([0.65, 0.65], [0., 0.70, 0.]), pose([1., 0.65], [0., 0.5, 0.25]),
            pose([4.8, 1.2], [0.04, 0.25, 0.5])]),
        PointyPencil => (6., 0.70, 0.9, 0.75, 0., [
            pose([0.55, 0.55], [0., 0.95, 0.]), pose([0.9, 0.5], [0., 0.8, 0.]),
            pose([5., 0.8], [0.05, 0.35, 0.45])]),
        ShadingPencil => (64./2.6, 0.63, 1.35, 0.65, 0., [
            pose([0.28, 0.28], [0., 0.7, 0.]), pose([1., 0.55], [0.04, 0.3, 0.3]),
            pose([2.6, 0.8], [0.10, 0.08, 0.6])]),
        Charcoal => (24., 0.85, 1.6, 0.55, 0., [
            pose([0.65, 0.65], [0.06, 0.5, 0.25]), pose([1., 0.65], [0.09, 0.15, 0.6]),
            pose([2., 0.8], [0.14, 0.07, 0.75])]),
        Chalk => (36./1.6_f32.hypot(0.85), 0.75, 1.4, 0.15, 0.24, [
            pose([0.55, 0.55], [0.09, 0.65, 0.]), pose([1., 0.8], [0.12, 0.65, 0.]),
            pose([1.6, 0.85], [0.16, 0.65, 0.])]),
        PastelBlock => (64./1.95, 0.85, 1.45, 0.60, 0.3, [
            pose([0.55, 0.55], [0.08, 0.65, 0.]), pose([1., 0.65], [0.10, 0.65, 0.]),
            pose([3.2, 0.55], [0.14, 0.65, 0.])]),
        Crayon => (32./2.6, 0.70, 0.8, 0.90, 0., [
            pose([0.65, 0.65], [0., 0.6, 0.15]), pose([1., 0.7], [0., 0.4, 0.3]),
            pose([2.6, 0.85], [0.03, 0.1, 0.6])]),
        _ => unreachable!(),
    };
    let writing = poses[1].radii[0].max(poses[1].radii[1]);
    let poses = poses.map(|mut p| { p.radii = p.radii.map(|v| v / writing); p });
    let minimum_size = match preset {
        ShadingPencil => 0.65, Charcoal => 0.55, Chalk|PastelBlock|Crayon => 0.75, _ => 0.15,
    };
    BrushSnapshot {
        diameter, flow, hardness: 1., seed: 0x4452_594d,
        color_rgba_linear: [0.014, 0.013, 0.012, 1.],
        grain: Some(crate::presets::paper_grain(1.)),
        contact: Some(BrushContact { paper, pressure_gain: 1.,
            dry: Some(BrushDryMedia { poses, softness: 0.05, loading, corner,
                relief_offset: if preset == PointyPencil { 0.65 } else { 0. } }), ..Default::default() }),
        rendering: BrushRendering { accumulation: BrushAccumulation::Flow, ..Default::default() },
        mappings: Arc::from([BrushMapping { output_scale: 1.-minimum_size, output_bias: minimum_size,
            ..BrushMapping::pressure_size() }]),
        ..Default::default()
    }
}

pub(crate) fn bristle_paintbrush() -> BrushSnapshot {
    BrushSnapshot {
        diameter: 460.,
        opacity: 1.,
        flow: 1.,
        color_rgba_linear: [0.12, 0.025, 0.012, 1.],
        seed: 0x4252_4953,
        grain: Some(crate::presets::paper_grain(0.6)),
        contact: Some(BrushContact {
            bristles: Some(BrushBristles::default()),
            paper: 1.,
            ..Default::default()
        }),
        shape: BrushShape {
            follow_twist: 1.,
            ..Default::default()
        },
        rendering: BrushRendering {
            accumulation: BrushAccumulation::Uniform,
            ..Default::default()
        },
        mappings: Arc::from([]),
        ..Default::default()
    }
}

pub(crate) const PEN_HARDNESS: f32 = 0.96;
pub(crate) const PEN_FLOW: f32 = 1.0;

pub(crate) fn contact_brush(preset: DefaultBrushPreset) -> BrushSnapshot {
    use DefaultBrushPreset::*;
    let mut model = BrushContact::default();
    let mut brush = BrushSnapshot {
        diameter: 18.0,
        color_rgba_linear: [0.006, 0.006, 0.006, 1.0],
        flow: PEN_FLOW,
        hardness: PEN_HARDNESS,
        spacing: 0.18,
        seed: 0x434f_4e54,
        rendering: BrushRendering {
            accumulation: BrushAccumulation::Uniform,
            ..Default::default()
        },
        mappings: Arc::from([BrushMapping {
            curve: BrushCurve {
                samples: std::array::from_fn(|i| (i as f32 / 15.0).powf(1.2)),
            },
            output_scale: 0.99,
            output_bias: 0.01,
            ..BrushMapping::pressure_size()
        }]),
        ..Default::default()
    };
    brush.stabilization.pressure_fall_micros = 80_000;
    match preset {
        GPen => {
            brush.stabilization.pressure_fall_micros = 34_133;
        }
        Monoline => {
            brush.mappings = Arc::from([]);
        }
        RoughGPen => {
            model.edge_roughness = 0.3;
            model.edge_scale = 1.7;
        }
        CalligraphyPen => {
            brush.diameter = 38.0;
            brush.aspect = 4.5;
            brush.angle_radians = -0.65;
            // A held, angled nib changes width with drawing direction.
            brush.shape.follow_direction = 0.0;
            model.edge_roughness = 0.03;
        }
        AntiquePen => {
            brush.diameter = 26.0;
            brush.aspect = 1.5;
            brush.angle_radians = -0.6;
            model.edge_roughness = 0.23;
            model.edge_scale = 2.8;
            model.fiber_strength = 0.3;
            model.fibers = 5.0;
            model.depletion = 0.004;
        }
        RealisticPen => {
            brush.diameter = 12.0;
            brush.hardness = 0.88;
            model.edge_roughness = 0.11;
            model.edge_scale = 0.9;
            model.pooling = 0.35;
        }
        WetInk => {
            brush.diameter = 38.0;
            brush.hardness = 0.8;
            brush.opacity = 1.0;
            model.edge_roughness = 0.16;
            model.edge_scale = 2.0;
            model.pooling = 1.0;
        }
        BlottyInk => {
            brush.diameter = 46.0;
            brush.hardness = 0.86;
            model.edge_roughness = 0.8;
            model.edge_scale = 8.0;
            model.pooling = 0.8;
        }
        BrushedInk => {
            brush.diameter = 52.0;
            brush.hardness = 0.84;
            brush.aspect = 0.72;
            brush.shape.follow_direction = 1.0;
            model.fibers = 22.0;
            model.fiber_strength = 0.94;
            model.depletion = 0.012;
            model.edge_roughness = 0.18;
            model.edge_scale = 2.0;
        }
        _ => unreachable!("only contact presets use the contact model"),
    }
    brush.contact = Some(model);
    brush
}

pub(crate) fn dry_material(preset: DefaultBrushPreset) -> BrushContact {
    use DefaultBrushPreset::*;
    match preset {
        Eraser | Airbrush | Marker => BrushContact::default(),
        Paintbrush | TexturedFlat | DualTexture => BrushContact {
            fibers: if preset == TexturedFlat { 32. } else { 19. },
            fiber_strength: 0.78,
            depletion: 0.001,
            edge_roughness: if preset == TexturedFlat { 0. } else { 0.08 },
            edge_scale: 1.7,
            paper: if preset == DualTexture { 0.72 } else { 0.15 },
            pressure_gain: 0.85,
            ..Default::default()
        },
        DryScumble => BrushContact {
            paper: 1.,
            pressure_gain: 0.25,
            fiber_strength: 0.35,
            fibers: 17.,
            edge_roughness: 0.15,
            ..Default::default()
        },
        // Glaze is a smooth translucent film; its soft edge supplies the wash
        // character without a second paper modulation inside the contact.
        TransparentGlaze => BrushContact::default(),
        _ => unreachable!("only continuous dry presets use this material"),
    }
}

#[cfg(test)]
mod vector_line_tests {
    use super::*;
    #[test]
    fn only_solid_round_contact_pens_draw_vector_lines() {
        assert!(contact_brush(DefaultBrushPreset::GPen).draws_vector_lines());
        assert!(contact_brush(DefaultBrushPreset::Monoline).draws_vector_lines());
        assert!(contact_brush(DefaultBrushPreset::Monoline).mappings.is_empty(), "Monoline keeps one width");
        for preset in [DefaultBrushPreset::RoughGPen, DefaultBrushPreset::CalligraphyPen, DefaultBrushPreset::BrushedInk, DefaultBrushPreset::WetInk] {
            assert!(!contact_brush(preset).draws_vector_lines(), "{preset:?}");
        }
        assert!(!BrushSnapshot::default().draws_vector_lines());
    }
}
