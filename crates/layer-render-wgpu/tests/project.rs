//! Compare a live, incrementally painted scene with a fresh GPU replay after
//! saving and reopening. Snapshots use renderer-retained immutable sources.
//! Readback is test-only, never part of project saving.
use layer_core::*;
use layer_engine::{
    CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::{CanvasRenderer, HostImage, ViewState};
use layer_render_wgpu::WgpuRasterizer;
use std::{collections::BTreeMap, sync::Arc};

type Engine = CanvasEngine<WgpuRasterizer>;
const SIZE: [u32; 2] = [384, 256]; // Crosses raster tile boundaries.

fn engine(project: &Project) -> (Engine, InputProducer<PenEvent>) {
    let mut gpu = WgpuRasterizer::new_headless().expect("physical GPU required");
    for (id, a) in &project.assets {
        gpu.prepare_asset(
            id,
            HostImage {
                width: a.extent[0],
                height: a.extent[1],
                stride: a.extent[0] * a.format.channels(),
                format: a.format,
                bytes: &a.bytes,
            },
        )
        .unwrap();
    }
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
fn image(engine: &mut Engine, time: u64) -> Vec<u8> {
    engine.render_frame_at(time).unwrap();
    let mut bytes = vec![0; (SIZE[0] * SIZE[1] * 4) as usize];
    engine
        .backend_mut()
        .copy_rgba8_srgb(&mut bytes, SIZE[0] as usize * 4)
        .unwrap();
    bytes
}
fn capture(name: &str, bytes: &[u8]) {
    if let Ok(path) = std::env::var("CAPY_PROJECT_CAPTURES") {
        std::fs::create_dir_all(&path).unwrap();
        let file =
            std::fs::File::create(std::path::Path::new(&path).join(format!("{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(file, SIZE[0], SIZE[1]);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(bytes)
            .unwrap();
    }
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
    }
}
fn operation(engine: &mut Engine, kind: LayerOperationKind, selection: Option<Selection>) {
    let mut coverage = LayerMask::reveal_all(LayerId(0), Point::default());
    coverage.default_coverage = if selection.is_some() { 0. } else { 1. };
    coverage.initial = selection;
    engine
        .append_layer_operation(
            LayerId(1),
            LayerOperation {
                after_stroke: engine.document().layer(LayerId(1)).unwrap().strokes.len(),
                coverage,
                kind,
            },
        )
        .unwrap();
    engine.render_frame_at(1_000_000_000).unwrap();
}
fn fixture(masked: bool) -> Project {
    let mut doc = Document::new("editable-project-test", SIZE[0], SIZE[1]);
    doc.layers[1].visible = false;
    let asset: AssetId = "fixture-source".into();
    doc.layers[0].asset = Some(asset.clone());
    let pixels: Vec<u8> = (0..SIZE[0] * SIZE[1])
        .flat_map(|i| {
            let x = i % SIZE[0];
            let y = i / SIZE[0];
            [
                (x % 256) as u8,
                (y % 256) as u8,
                118,
                if x < 16 || y < 16 { 0 } else { 140 },
            ]
        })
        .collect();
    if masked {
        let group_id = doc.allocate_layer_id();
        doc.layers[0].properties.parent = Some(group_id);
        doc.layers[0].properties.offset = Point { x: 3., y: -2. };
        let mut group = Layer::paint(group_id, "Group");
        group.kind = LayerKind::Group;
        group.properties.offset = Point { x: -3., y: 2. };
        doc.layers.insert(0, group);
        let mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point::default());
        doc.layers
            .iter_mut()
            .find(|l| l.id == LayerId(1))
            .unwrap()
            .mask = Some(mask);
        for (filter, clipped) in [
            ("gaussian_blur", true),
            ("curves", true),
            ("heat_haze", true),
            ("gradient_map", false),
        ] {
            let definition = bundled_effect_catalog().get(filter).unwrap();
            let mut layer = Layer::paint(doc.allocate_layer_id(), definition.label());
            layer.kind = LayerKind::Effect;
            layer.properties.parent = Some(group_id);
            layer.properties.clipped = clipped;
            let mut effect = definition.preview().unwrap();
            if effect.program.time {
                effect.set("animate", EffectValue::Toggle(true)).unwrap();
            }
            layer.effect = Some(Arc::new(effect));
            doc.layers.insert(1, layer);
        }
        doc.reference_layers.insert(LayerId(1));
    }
    let assets = BTreeMap::from([(
        asset,
        ProjectAsset {
            extent: SIZE,
            format: ProjectAssetFormat::Rgba8Srgb,
            bytes: pixels.into(),
        },
    )]);
    Project::snapshot(&doc, &assets).unwrap()
}

#[test]
fn project_reopen_matches_live_gpu_and_subsequent_wet_paint() {
    for masked in [false, true] {
        let initial = fixture(masked);
        let (mut live, mut input) = engine(&initial);
        draw(
            &mut live,
            &mut input,
            DefaultBrushPreset::GPen,
            [0.1, 0.2, 0.8, 0.8],
            65.,
            100_000_000,
        );
        {
            let checkpoint =
                Project::snapshot_with(live.document(), |id| live.backend().source_asset(id))
                    .unwrap();
            let (mut replay, _) = engine(&checkpoint);
            let expected = image(&mut live, 190_000_000);
            let actual = image(&mut replay, 190_000_000);
            assert_eq!(actual, expected, "G-Pen replay, masked={masked}");
        }
        draw(
            &mut live,
            &mut input,
            DefaultBrushPreset::WatercolorWash,
            [0.8, 0.1, 0.15, 0.8],
            120.,
            200_000_000,
        );
        {
            let checkpoint =
                Project::snapshot_with(live.document(), |id| live.backend().source_asset(id))
                    .unwrap();
            let (mut replay, _) = engine(&checkpoint);
            let expected = image(&mut live, 290_000_000);
            let actual = image(&mut replay, 290_000_000);
            assert_eq!(
                actual.iter().zip(&expected).filter(|(a, b)| a != b).count(),
                0,
                "watercolor replay, masked={masked}"
            );
        }
        draw(
            &mut live,
            &mut input,
            DefaultBrushPreset::WetWatercolor,
            [0.1, 0.7, 0.2, 0.7],
            138.,
            300_000_000,
        );
        if masked {
            // A painted mask survives Apply mask as immutable replay history.
            live.apply_edit(Edit::SetMaskTarget(true)).unwrap();
            live.set_tool(StrokeTool::Eraser);
            draw(
                &mut live,
                &mut input,
                DefaultBrushPreset::GPen,
                [1.; 4],
                98.,
                400_000_000,
            );
            live.set_tool(StrokeTool::Brush);
            live.apply_edit(Edit::SetMaskTarget(false)).unwrap();
            let mut layer = live.document().layer(LayerId(1)).unwrap().clone();
            let applied = layer.mask.take().unwrap();
            layer.operations.push(LayerOperation {
                after_stroke: layer.strokes.len(),
                coverage: applied,
                kind: LayerOperationKind::ApplyMask,
            });
            layer.mask = Some(LayerMask::reveal_all(
                live.allocate_layer_id(),
                Point { x: 2., y: 1. },
            ));
            layer.mask.as_mut().unwrap().initial = Some(
                Selection::polygon(vec![
                    Point { x: 20., y: 15. },
                    Point { x: 365., y: 40. },
                    Point { x: 330., y: 240. },
                    Point { x: 20., y: 230. },
                ])
                .unwrap(),
            );
            layer.mask.as_mut().unwrap().default_coverage = 0.;
            live.apply_edit(Edit::ReplaceLayer(Box::new(layer)))
                .unwrap();
            let selection = Selection::polygon(vec![
                Point { x: 120., y: 30. },
                Point { x: 330., y: 30. },
                Point { x: 300., y: 180. },
            ])
            .unwrap();
            operation(
                &mut live,
                LayerOperationKind::Gradient {
                    start: Point { x: 120., y: 30. },
                    end: Point { x: 300., y: 180. },
                    colors: [[0.9, 0.4, 0.1, 0.5], [0.; 4]],
                    radial: true,
                    alpha_locked: false,
                },
                Some(selection),
            );
            operation(
                &mut live,
                LayerOperationKind::Figure(Figure {
                    shape: FigureShape::Ellipse,
                    paint: FigurePaint::Both,
                    start: Point { x: 240., y: 140. },
                    end: Point { x: 305., y: 210. },
                    width: 5.,
                    colors: [[0.1, 0.7, 0.9, 0.9], [0.5, 0.1, 0.8, 0.4]],
                    alpha_locked: false,
                    erase: false,
                }),
                None,
            );
            operation(
                &mut live,
                LayerOperationKind::Transform(ImageTransform {
                    affine: Affine::translation(Point { x: 7.5, y: -4.25 }),
                    interpolation: Interpolation::Linear,
                }),
                None,
            );
        }
        let project =
            Project::snapshot_with(live.document(), |id| live.backend().source_asset(id)).unwrap();
        let mut encoded = Vec::new();
        project.write(&mut encoded).unwrap();
        let decoded = Project::read(encoded.as_slice(), ProjectLimits::default()).unwrap();
        assert_eq!(decoded, project);
        let (mut restored, mut restored_input) = engine(&decoded);
        for time in [1_000_000_000, 1_750_000_000] {
            let expected = image(&mut live, time);
            let actual = image(&mut restored, time);
            let difference = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
            assert_eq!(
                difference, 0,
                "reopen changed {difference} channels; masked={masked}, time={time}"
            );
            assert!(
                actual.chunks_exact(4).any(|p| p[3] == 0),
                "fixture has transparent pixels"
            );
            assert!(
                actual
                    .chunks_exact(4)
                    .any(|p| p[3] > 0 && p[0] > 0 && p[0] < 255),
                "fixture has painted pixels"
            );
            capture(&format!("project-{masked}-{time}"), &actual);
        }
        // Proves the stored history restores material/wet channels, not merely
        // a flattened screenshot that looks correct until the next brush stroke.
        for (canvas, events) in [
            (&mut live, &mut input),
            (&mut restored, &mut restored_input),
        ] {
            draw(
                canvas,
                events,
                DefaultBrushPreset::WetWatercolor,
                [0.6, 0.1, 0.8, 0.8],
                150.,
                2_000_000_000,
            );
        }
        let expected = image(&mut live, 2_250_000_000);
        let actual = image(&mut restored, 2_250_000_000);
        assert_eq!(
            actual
                .iter()
                .zip(expected)
                .filter(|(a, b)| **a != *b)
                .count(),
            0,
            "continued painting differs; masked={masked}"
        );
    }
}
