//! Compare live painting with exact raster restoration after saving and reopening.
//! File workers await queue-ordered tile capture while input remains responsive.
use layer_core::*;
use layer_engine::{
    CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::WgpuRasterizer;
use std::sync::Arc;

type Engine = CanvasEngine<WgpuRasterizer>;
const SIZE: [u32; 2] = [384, 256]; // Crosses raster tile boundaries.

fn engine(project: &Project) -> (Engine, InputProducer<PenEvent>) {
    let gpu = WgpuRasterizer::new_native_headless(project.document.color)
        .expect("physical GPU required");
    let (producer, consumer) = input_queue(64);
    let view = ViewState {
        width_px: SIZE[0],
        height_px: SIZE[1],
        document_to_surface: Affine::IDENTITY.0,
        background_rgba_linear: [0.; 4],
    };
    let mut engine = CanvasEngine::new(
        gpu,
        project.document.clone(),
        consumer,
        view,
        ViewTransform::IDENTITY,
    )
    .unwrap();
    engine.render_frame_at(0).unwrap();
    (engine, producer)
}

#[test]
fn source_backed_save_reopen_preserves_original_and_edited_tiles() {
    use layer_core::color::{SampleDepth, source::*};
    let mut builder = SourceBuilder::new(SIZE, SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::U16,
        profile: Default::default(), profile_assumed: false,
    }, 16 * 1024 * 1024).unwrap();
    for y in 0..SIZE[1] {
        let row: Vec<_> = (0..SIZE[0]).flat_map(|x| {
            [x.wrapping_mul(617) as u16, y.wrapping_mul(251) as u16, (x ^ y) as u16, 65535]
                .into_iter().flat_map(u16::to_le_bytes)
        }).collect();
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    let mut document = Document::new("retained16 source in sRGB8 working document", SIZE[0], SIZE[1]);
    document.layers[0].source = Some(source.clone());
    let project = Project { document };
    let (mut live, mut input) = engine(&project);
    let original = image(&mut live, 0);
    draw(&mut live, &mut input, DefaultBrushPreset::GPen, [1., 0., 0., 0.5], 100., 1_000_000);
    let painted = image(&mut live, 100_000_000);
    assert!(painted != original);
    let mut archive = Vec::new();
    let snapshot = Project::snapshot(live.document()).unwrap();
    snapshot.write(&mut archive).unwrap();
    let loaded = Project::read(archive.as_slice(), Default::default()).unwrap();
    assert_eq!(loaded.document.layers[0].source.as_ref().unwrap(), &source);
    let (mut reopened, _) = engine(&loaded);
    let actual = image(&mut reopened, 0);
    assert_eq!(actual.iter().zip(&painted).enumerate().find(|(_, (a,b))| a != b), None);
    live.undo().unwrap();
    let restored = image(&mut live, 200_000_000);
    assert_eq!(restored.iter().zip(&original).enumerate().find(|(_, (a,b))| a != b), None);
    live.redo().unwrap();
    let restored = image(&mut live, 300_000_000);
    assert_eq!(restored.iter().zip(&painted).enumerate().find(|(_, (a,b))| a != b), None);
}
fn image(engine: &mut Engine, time: u64) -> Vec<u8> {
    engine.render_frame_at(time).unwrap();
    let mut bytes = vec![0; (SIZE[0] * SIZE[1] * 4) as usize];
    engine
        .backend_mut()
        .copy_rgba8_srgb(&mut bytes, SIZE[0] as usize * 4)
        .unwrap();
    bytes
}
fn draw(
    engine: &mut Engine,
    input: &mut InputProducer<PenEvent>,
    preset: DefaultBrushPreset,
    color: [f32; 4],
    y: f32,
    start: u64,
) {
    let mut brush = default_brush(preset);
    brush.diameter = 56.;
    brush.color_rgba_linear = color;
    engine.set_brush(brush).unwrap();
    for i in 0..10 {
        let timestamp_ns = start + i * 8_000_000;
        input
            .push(PenEvent {
                device_id: 1,
                sequence: i,
                timestamp_ns,
                view_revision: 0,
                surface_position: Point {
                    x: 40. + i as f32 * 32.,
                    y: y + (i as f32 * 0.8).sin() * 20.,
                },
                pressure: 0.3 + i as f32 * 0.07,
                tilt_radians: [0.2, -0.1],
                twist_radians: 0.3,
                distance: 0.,
                phase: if i == 0 {
                    PenPhase::Down
                } else if i == 9 {
                    PenPhase::Up
                } else {
                    PenPhase::Move
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            })
            .unwrap();
        engine.render_frame_at(timestamp_ns).unwrap();
        while engine.has_pending_input() {
            std::thread::yield_now();
            engine.render_frame_at(timestamp_ns).unwrap();
        }
    }
}
fn fixture() -> Project {
    let mut doc = Document::new("editable-project-test", SIZE[0], SIZE[1]);
    doc.layers[1].visible = false;
    doc.layers[0].source = Some(color::source::rgba8_source(SIZE, |x, y| {
        [
            (x % 256) as u8,
            (y % 256) as u8,
            118,
            if x < 16 || y < 16 { 0 } else { 140 },
        ]
    }));
    Project::snapshot(&doc).unwrap()
}

#[test]
fn every_brush_preset_paints_survives_save_reopen_and_exact_undo_redo() {
    for masked in [false, true] {
        preset_history(masked);
    }
}

fn preset_history(masked: bool) {
    let mut initial = fixture();
    if masked {
        let mut mask =
            LayerMask::reveal_all(initial.document.allocate_layer_id(), Point::default());
        mask.default_coverage = 0.;
        initial.document.layers[0].mask = Some(mask);
    }
    let (mut live, mut input) = engine(&initial);
    if masked {
        live.apply_edit(Edit::SetMaskTarget(true)).unwrap();
    }
    let mut before = image(&mut live, 0);
    use DefaultBrushPreset::*;
    let presets = CONTACT_BRUSH_PRESETS.into_iter().chain([
        Spray, WetRound, MultiplyGlaze, OpaqueGouache, WatercolorWash, WetWatercolor,
        LoadedOil, PaletteKnife, Smudge, NaturalBlender, LiquifyPush, LiquifyTwirl,
        LiquifyTwirlClockwise, LiquifyPinch, LiquifyExpand, LiquifyCrystals,
    ]);
    for (index, preset) in presets.enumerate() {
        let time = (index as u64 + 1) * 100_000_000;
        draw(
            &mut live,
            &mut input,
            preset,
            [0.03, 0.02, 0.01, 0.8],
            40. + (index % 4) as f32 * 52.,
            time,
        );
        let expected = image(&mut live, time + 80_000_000);
        assert_ne!(before, expected, "{preset:?} must leave a visible mark");
        let checkpoint = Project::snapshot(live.document()).unwrap();
        let mut archive = Vec::new();
        checkpoint.write(&mut archive).unwrap();
        let decoded = Project::read(archive.as_slice(), ProjectLimits::default()).unwrap();
        let (mut reopened, _) = engine(&decoded);
        assert_eq!(
            image(&mut reopened, time + 80_000_000),
            expected,
            "{preset:?}: archive roundtrip"
        );
        assert!(live.undo().unwrap());
        assert_eq!(
            image(&mut live, time + 81_000_000),
            before,
            "{preset:?}: undo"
        );
        assert!(live.redo().unwrap());
        assert_eq!(
            image(&mut live, time + 82_000_000),
            expected,
            "{preset:?}: redo"
        );
        before = expected;
    }
}

