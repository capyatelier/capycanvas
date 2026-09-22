//! Built-in media using the shared GPU contact model.
use crate::*;
use std::sync::Arc;

pub const CONTACT_BRUSH_PRESETS: [DefaultBrushPreset; 22] = [
    DefaultBrushPreset::Pencil,
    DefaultBrushPreset::PointyPencil,
    DefaultBrushPreset::ShadingPencil,
    DefaultBrushPreset::Charcoal,
    DefaultBrushPreset::GPen,
    DefaultBrushPreset::RoughGPen,
    DefaultBrushPreset::CalligraphyPen,
    DefaultBrushPreset::AntiquePen,
    DefaultBrushPreset::RealisticPen,
    DefaultBrushPreset::WetInk,
    DefaultBrushPreset::BlottyInk,
    DefaultBrushPreset::BrushedInk,
    DefaultBrushPreset::Eraser,
    DefaultBrushPreset::Paintbrush,
    DefaultBrushPreset::Airbrush,
    DefaultBrushPreset::Chalk,
    DefaultBrushPreset::Marker,
    DefaultBrushPreset::DualTexture,
    DefaultBrushPreset::TexturedFlat,
    DefaultBrushPreset::DryScumble,
    DefaultBrushPreset::PastelBlock,
    DefaultBrushPreset::TransparentGlaze,
];

pub(crate) fn contact_brush(preset: DefaultBrushPreset) -> BrushSnapshot {
    use DefaultBrushPreset::*;
    let pencil = matches!(preset, Pencil | PointyPencil | ShadingPencil | Charcoal);
    let mut model = BrushContact::default();
    let mut brush = BrushSnapshot {
        schema_version: 5,
        diameter: 18.0,
        color_rgba_linear: [0.006, 0.006, 0.006, 1.0],
        flow: 1.0,
        hardness: 0.96,
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
    if pencil {
        brush.color_rgba_linear = [0.018, 0.017, 0.016, 1.0];
        brush.diameter = 14.0;
        brush.hardness = 0.82;
        brush.flow = 0.65;
        brush.spacing = 0.13;
        brush.rendering.accumulation = BrushAccumulation::Flow;
        brush.grain = Some(BrushGrain {
            asset: AssetId::from(CONTACT_PAPER_TEXTURE_ASSET),
            behavior: BrushGrainBehavior::Canvas,
            scale: 1.8,
            depth: 1.0,
            rotation_radians: 0.0,
            offset_jitter: 0.0,
        });
        brush.mappings = Arc::from([BrushMapping {
            output_scale: 0.55,
            output_bias: 0.45,
            ..BrushMapping::pressure_size()
        }]);
        model.paper = 1.0;
        model.pressure_gain = 0.9;
        model.tilt_spread = 2.0;
        model.tilt_shading = 0.7;
    } else {
        // Limit falling pressure in the existing causal stabilization path.
        // No tool-settings control is exposed for this yet.
        brush.stabilization.pressure_fall_micros = 80_000;
    }
    match preset {
        Pencil => {}
        PointyPencil => {
            brush.diameter = 10.0;
            brush.hardness = 0.82;
            brush.flow = 1.0;
            model.tip_bias = 0.78;
            model.tilt_spread = 1.8;
        }
        ShadingPencil => {
            brush.diameter = 48.0;
            brush.hardness = 0.25;
            brush.flow = 0.55;
            brush.aspect = 2.5;
            model.tilt_spread = 2.4;
            model.tip_bias = 0.25;
            brush.rendering.accumulation = BrushAccumulation::Uniform;
        }
        Charcoal => {
            brush.diameter = 64.0;
            brush.hardness = 0.7;
            brush.flow = 1.0;
            brush.aspect = 1.6;
            brush.grain.as_mut().unwrap().scale = 0.75;
            model.edge_roughness = 0.24;
            model.edge_scale = 3.0;
            model.pressure_gain = 1.0;
            model.tilt_spread = 1.1;
            model.tilt_shading = 0.45;
        }
        GPen => {
            brush.stabilization.pressure_fall_micros = 34_133;
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
        Chalk | PastelBlock => BrushContact {
            paper: 1.,
            pressure_gain: 0.75,
            edge_roughness: if preset == Chalk { 0.12 } else { 0.06 },
            edge_scale: 2.5,
            tilt_spread: 0.4,
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
