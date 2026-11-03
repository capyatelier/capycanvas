use layer_core::*;
use layer_engine::{
    CanvasEngine, InputProducer, PenEvent, PenPhase, ToolKind, SampleFlags, ViewTransform,
    InstantFeedbackConfig,
};
use layer_render::{CanvasRenderer, ViewState};
use std::sync::Arc;

pub fn pen_sample(index: u64, last: u64, timestamp_ns: u64, surface_position: Point) -> PenEvent {
    PenEvent {
        device_id: 1,
        sequence: index,
        timestamp_ns,
        view_revision: 0,
        surface_position,
        pressure: 0.8,
        tilt_radians: [0., 0.],
        twist_radians: 0.,
        distance: 0.,
        phase: if index == 0 {
            PenPhase::Down
        } else if index == last {
            PenPhase::Up
        } else {
            PenPhase::Move
        },
        tool: ToolKind::Pen,
        flags: SampleFlags::PRIMARY,
    }
}

pub(crate) fn striped_paper(document: &mut Document, paper: authored::OccurrenceHandle) {
    use color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Gray,
        depth: color::SampleDepth::U8,
        profile: Default::default(),
        profile_assumed: false,
    };
    let mut builder = SourceBuilder::new([1, 256], interpretation, 1 << 20).unwrap();
    for y in 0..256 {
        builder
            .push_row(&[if y % 16 < 8 { 255 } else { 0 }])
            .unwrap();
    }
    let mut source = builder.finish().unwrap();
    source.resolution = Some(ImageResolution::ppi(300));
    let effect = document.scene().effect_handle(paper).unwrap();
    let application = document.artwork.effects.get_mut(effect).unwrap();
    let index = application
        .program
        .parameters
        .iter()
        .position(|p| p.key.as_ref() == "texture")
        .unwrap();
    application.values[index] =
        EffectValue::Texture(TextureSource::Image(Image::new(Arc::new(source))));
}

pub(crate) fn dry_contact_contains(end: layer_render::Dab, corner: f32, x: f32, y: f32) -> bool {
    let delta = [x - end.center.x, y - end.center.y];
    let [cos, sin] = end.rotation;
    let o = [
        (delta[0] * cos + delta[1] * sin) / end.radii[0],
        (-delta[0] * sin + delta[1] * cos) / end.radii[1],
    ];
    let q = if corner > 0. {
        let rim = corner * end.radii[0].min(end.radii[1]);
        let rim = [rim / end.radii[0], rim / end.radii[1]];
        let edge = [
            (o[0].abs() - (1. - rim[0])).max(0.) / rim[0],
            (o[1].abs() - (1. - rim[1])).max(0.) / rim[1],
        ];
        (edge[0] + edge[1]).powi(2)
    } else {
        o[0] * o[0] + o[1] * o[1]
    };
    q <= 1.
}

pub(crate) fn owned_contact_engine<B: CanvasRenderer>(
    backend: B,
    preset: DefaultBrushPreset,
    diameter: f32,
    strength: f32,
) -> (CanvasEngine<B>, InputProducer<PenEvent>)
where
    B::Error: std::fmt::Debug,
{
    let mut brush = default_brush(preset);
    brush.diameter = diameter;
    brush.stabilization.strength = strength;
    let mut document = Document::new(
        PortableId::random(),
        256,
        256,
        DocumentNames {
            paint: "Ink".into(),
            paper: "Paper".into(),
        },
    );
    let paper = document
        .scene()
        .order()
        .iter()
        .copied()
        .find(|&h| document.scene().occurrence(h).unwrap().name.as_ref() == "Paper")
        .unwrap();
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    striped_paper(&mut document, paper);
    let (input, consumer) = layer_engine::input_queue(64);
    let mut engine = CanvasEngine::new(
        backend,
        document,
        consumer,
        ViewState {
            width_px: 256,
            height_px: 256,
            document_to_surface: [1., 0., 0., 1., 0., 0.],
        },
        ViewTransform::IDENTITY,
    )
    .unwrap();
    engine.set_brush(brush).unwrap();
    engine
        .set_instant_feedback(InstantFeedbackConfig {
            enabled: true,
            use_platform_prediction: false,
            ..Default::default()
        })
        .unwrap();
    engine.render_frame_at(0).unwrap();
    (engine, input)
}
