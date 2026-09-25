use super::*;
use layer_core::color::{SampleDepth, RgbSpace};
use layer_engine::{
    CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::ViewState;

fn view() -> ViewState {
    ViewState {
        width_px: 256,
        height_px: 256,
        document_to_surface: [1., 0., 0., 1., 0., 0.],
        background_rgba_linear: [0.; 4],
    }
}
fn flush(engine: &mut CanvasEngine<WgpuRasterizer>) {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        engine.render_frame().unwrap();
        if !engine.has_pending_input() && !engine.has_pending_document_edits() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
}
fn stroke(
    engine: &mut CanvasEngine<WgpuRasterizer>,
    input: &mut InputProducer<PenEvent>,
    sequence: u64,
    x: f32,
) {
    for (i, phase) in [PenPhase::Down, PenPhase::Move, PenPhase::Up]
        .into_iter()
        .enumerate()
    {
        input
            .push(PenEvent {
                device_id: 1,
                sequence: sequence + i as u64,
                timestamp_ns: (sequence + i as u64) * 10_000_000,
                view_revision: 0,
                surface_position: layer_core::Point {
                    x: x + i as f32 * 9.,
                    y: 80.,
                },
                pressure: 0.37,
                tilt_radians: [0.; 2],
                twist_radians: 0.,
                distance: 0.,
                phase,
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            })
            .unwrap();
        flush(engine);
    }
}
fn working(r: &WgpuRasterizer, id: LayerId) -> BTreeMap<TileKey, Vec<u8>> {
    r.raster_textures(id)
        .0
        .into_iter()
        .map(|(key, texture)| (key, crate::layer_tests::page_bytes(r, texture)))
        .collect()
}
fn backing(root: &RasterRevision) -> BTreeMap<TileKey, Vec<u8>> {
    root.wait_data()
        .unwrap()
        .tiles
        .iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode().unwrap()))
        .collect()
}
fn engine(
    document: layer_core::Document,
) -> (InputProducer<PenEvent>, CanvasEngine<WgpuRasterizer>) {
    let r = WgpuRasterizer::new_native_headless(document.color).unwrap();
    let (input, consumer) = input_queue(64);
    let mut engine =
        CanvasEngine::new(r, document, consumer, view(), ViewTransform::IDENTITY).unwrap();
    let brush = layer_core::BrushSnapshot {
        color_rgba_linear: [0.012345, 0.234567, 0.678901, 0.12345],
        color_dynamics: layer_core::BrushColorDynamics {
            secondary_color_rgba_linear: [0.4, 0.18, 0.76, 0.23],
            stamp_hue_jitter: 0.17,
            stroke_saturation_jitter: 0.12,
            stamp_secondary_jitter: 0.37,
            ..Default::default()
        },
        ..Default::default()
    };
    engine.set_brush(brush).unwrap();
    flush(&mut engine);
    (input, engine)
}

#[test]
fn native_extended_fill_gradient_and_figure_pixels_survive_history_and_save() {
    use layer_core::{Affine, Figure, FigurePaint, FigureShape, LayerMask, LayerOperation, LayerOperationKind, Point};
    use layer_core::color::{RgbColor, f16};
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16] {
            let color = DocumentColor { space, depth };
            let mut document = layer_core::Document::new("portable paint", 384, 128);
            document.color = color;
            let id = document.layers[0].id;
            let (_, mut live) = engine(document);
            let empty = backing(&live.document().layers[0].raster);
            let colors = if depth.is_float() {
                [[8., -0.125, 2., 0.625], [-0.25, 3., 0.5, 0.375]]
            } else {
                [[1., 0., 0., 0.625], [0., 1., 0.5, 0.375]].map(|p|
                    RgbColor::new(RgbSpace::DisplayP3, p).unwrap().linear_in(space).unwrap())
            };
            let start = Point { x: 64., y: 32. };
            let end = Point { x: 320., y: 96. };
            let mut kinds = vec![LayerOperationKind::Fill { color: colors[0], alpha_locked: false }];
            for radial in [false, true] {
                for transparent in [false, true] {
                    let mut colors = colors;
                    if transparent { colors[1][3] = 0.; }
                    kinds.push(LayerOperationKind::Gradient { start, end, colors, radial, alpha_locked: false });
                }
            }
            kinds.push(LayerOperationKind::Figure(Figure {
                shape: FigureShape::Rectangle, paint: FigurePaint::Fill,
                start, end, width: 4., colors, alpha_locked: false, erase: false,
            }));
            kinds.push(LayerOperationKind::Figure(Figure {
                shape: FigureShape::Rectangle, paint: FigurePaint::Both,
                start, end, width: 4., colors, alpha_locked: false, erase: false,
            }));
            for kind in kinds {
                live.append_layer_operation(id, LayerOperation {
                    placement: Affine::IDENTITY,
                    coverage: LayerMask::reveal_all(LayerId(20), Point::default()),
                    kind: kind.clone(),
                }).unwrap();
                flush(&mut live);
                let pixels = backing(&live.document().layers[0].raster);
                assert_ne!(pixels, empty, "{color:?} {kind:?}");
                // Independent straight-linear reference, including both sides
                // of the 256px page boundary and outside the figure.
                for (x, y) in [(16, 64), (128, 64), (300, 64), (368, 64)] {
                    let expected = match &kind {
                        LayerOperationKind::Fill { color, .. } => *color,
                        LayerOperationKind::Gradient { colors: [a, b], radial, .. } => {
                            let p = [x as f64 + 0.5 - 64., y as f64 + 0.5 - 32.];
                            let t = (if *radial { p[0].hypot(p[1]) / 256f64.hypot(64.) }
                                else { (p[0]*256. + p[1]*64.) / (256.*256. + 64.*64.) }).clamp(0., 1.);
                            let alpha = (1.-t)*f64::from(a[3]) + t*f64::from(b[3]);
                            std::array::from_fn(|i| if i == 3 { alpha as f32 } else if alpha == 0. { 0. }
                                else { (((1.-t)*f64::from(a[i])*f64::from(a[3]) + t*f64::from(b[i])*f64::from(b[3])) / alpha) as f32 })
                        },
                        LayerOperationKind::Figure(f) => if (64..320).contains(&x) {
                            colors[usize::from(f.paint == FigurePaint::Both)]
                        } else { [0.; 4] },
                        _ => unreachable!(),
                    };
                    let key = TileKey { plane: layer_core::raster::RasterPlane::Color, coordinate: [x / 256, y / 256] };
                    let bpp = depth.bytes() * 4;
                    let offset = ((y % 256 * 256 + x % 256) as usize) * bpp;
                    let absent = vec![0; bpp];
                    let bytes = pixels.get(&key).map_or(absent.as_slice(), |p| &p[offset..offset+bpp]);
                    for i in 0..4 {
                        if depth == SampleDepth::F16 {
                            let actual = f16::from_bits(u16::from_le_bytes(bytes[2*i..2*i+2].try_into().unwrap()));
                            let reference = f16::from_f32(expected[i]);
                            assert_eq!(actual, reference, "{color:?} {kind:?} at {x},{y}/{i}");
                        } else {
                            let max = if depth == SampleDepth::U8 { 255. } else { 65535. };
                            let encoded = if i == 3 { f64::from(expected[i]) } else { space.encode(f64::from(expected[i])) };
                            let reference = (encoded.clamp(0., 1.)*max).round() as u16;
                            let actual = if depth == SampleDepth::U8 { u16::from(bytes[i]) }
                                else { u16::from_le_bytes(bytes[2*i..2*i+2].try_into().unwrap()) };
                            assert!(actual.abs_diff(reference) <= 1, "{color:?} {kind:?} at {x},{y}/{i}: {actual} vs {reference}");
                        }
                    }
                }
                assert!(live.undo().unwrap()); flush(&mut live);
                assert_eq!(backing(&live.document().layers[0].raster), empty);
                assert!(live.redo().unwrap()); flush(&mut live);
                assert_eq!(backing(&live.document().layers[0].raster), pixels);
                let project = layer_core::Project { document: live.document().clone(), assets: Default::default() };
                let mut bytes = Vec::new(); project.write(&mut bytes).unwrap();
                let loaded = layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap();
                assert_eq!(loaded.document.color, color);
                assert_eq!(backing(&loaded.document.layers[0].raster), pixels);
                assert!(live.undo().unwrap()); flush(&mut live);
            }
        }
    }
}

#[test]
fn native_engine_paint_undo_save_reopen_and_device_replacement_share_canonical_samples() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            let color = DocumentColor { space, depth };
            let mut document = layer_core::Document::new("native workflow", 256, 256);
            document.color = color;
            let id = document.layers[0].id;
            let (mut input, mut live) = engine(document);
            stroke(&mut live, &mut input, 1, 60.);
            let first = live.document().layers[0].raster.clone();
            let native_first = backing(&first);
            assert!(!native_first.is_empty(), "{color:?}");
            let working_first = working(live.backend(), id);
            stroke(&mut live, &mut input, 10, 68.);
            let second = live.document().layers[0].raster.clone();
            let native_second = backing(&second);
            let working_second = working(live.backend(), id);
            assert_ne!(native_first, native_second, "{color:?}");
            assert!(live.undo().unwrap());
            flush(&mut live);
            assert_eq!(backing(&live.document().layers[0].raster), native_first);
            assert_eq!(working(live.backend(), id), working_first, "undo {color:?}");
            assert!(live.redo().unwrap());
            flush(&mut live);
            assert_eq!(
                working(live.backend(), id),
                working_second,
                "redo {color:?}"
            );
            let project = layer_core::Project {
                document: live.document().clone(),
                assets: Default::default(),
            };
            let mut bytes = Vec::new();
            project.write(&mut bytes).unwrap();
            let loaded = layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap();
            assert_eq!(loaded.document.color, color);
            let (mut reopened_input, mut reopened) = engine(loaded.document);
            assert_eq!(
                backing(&reopened.document().layers[0].raster),
                native_second
            );
            assert_eq!(
                working(reopened.backend(), id),
                working_second,
                "reopen {color:?}"
            );
            stroke(&mut live, &mut input, 20, 73.);
            stroke(&mut reopened, &mut reopened_input, 20, 73.);
            let third = backing(&live.document().layers[0].raster);
            assert_eq!(
                third,
                backing(&reopened.document().layers[0].raster),
                "continue {color:?}"
            );
            assert_eq!(working(live.backend(), id), working(reopened.backend(), id));
            let checkpoint = live.checkpoint();
            let before_recovery = working(live.backend(), id);
            let replacement = WgpuRasterizer::new_native_headless(color).unwrap();
            let retired = live.replace_backend(replacement).unwrap();
            retired.device.destroy();
            drop(retired);
            flush(&mut live);
            assert_eq!(live.checkpoint(), checkpoint);
            assert_eq!(backing(&live.document().layers[0].raster), third);
            assert_eq!(
                working(live.backend(), id),
                before_recovery,
                "replacement {color:?}"
            );
            assert!(live.undo().unwrap());
            flush(&mut live);
            assert_eq!(backing(&live.document().layers[0].raster), native_second);
        }
    }
}

#[test]
fn saturated_uniform_contacts_match_the_full_evaluator_exactly() {
    // Compile the current evaluator with the shortcut disabled. Compare native
    // Float32 working pixels, coverage and prediction, not just an 8-bit export.
    fn reference(r: &mut WgpuRasterizer) {
        r.pipelines.dry_in_place = None;
        let source = include_str!("../../material_brush.wgsl");
        assert!(source.contains("if stroke_coverage >= ceiling"));
        let source = source.replace("if stroke_coverage >= ceiling", "if false");
        let device = r.device.clone();
        let shader = Deferred::new(move || device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("full uniform contact reference"),
            source: wgpu::ShaderSource::Wgsl(dry_material::shader_source(&device, false, &source)),
        }));
        r.pipelines.dry_material = Some(dry_material::Pipelines::new(&r.device, &PipelineLayouts {
            style: &r.style_layout, texture: &r.texture_layout,
            advanced_texture: &r.advanced_texture_layout, target: &r.target_layout,
            material: &r.material_layout, edge: &r.edge_layout,
            watercolor: &r.watercolor_layout, transport: &r.transport_layout,
        }, &shader, false));
    }
    fn state(r: &WgpuRasterizer) -> Vec<([u32; 2], Vec<u8>)> {
        r.paint_layers.iter().flat_map(|layer| layer.pages.iter()
            .map(|page| (page.coordinate, &page.active().texture))
            .chain(layer.coverage_pages.iter().map(|page| (page.coordinate, &page.active().texture))))
            .chain(r.preview_pages.iter().map(|page| (page.coordinate, &page.active().texture)))
            .map(|(coordinate, texture)| (coordinate, crate::layer_tests::page_bytes(r, texture)))
            .collect()
    }
    use layer_core::DefaultBrushPreset::{GPen, Marker};
    for (preset, depth, alpha, flow, large) in [
        (GPen, SampleDepth::U8, 1., 1., false),
        (GPen, SampleDepth::U16, 0.37, 1., false),
        (GPen, SampleDepth::F16, 1., 1., false),
        (GPen, SampleDepth::U8, 1., 1., true),
        (Marker, SampleDepth::U8, 0.4, 0.3, false),
        (Marker, SampleDepth::U16, 0.82, 0.72, false),
        (Marker, SampleDepth::F16, 1., 1., false),
    ] {
        let extent = if large { [4096, 3072] } else { [512, 384] };
        let mut document = layer_core::Document::new("saturated contact oracle", extent[0], extent[1]);
        document.color.depth = depth;
        let (mut input, mut live) = engine(document.clone());
        let (mut full_input, mut full) = engine(document);
        reference(full.backend_mut());
        let mut brush = layer_core::default_brush(preset);
        brush.flow = flow;
        brush.diameter = if large { 2048. } else { 224. };
        brush.color_rgba_linear = if large { [0.006, 0.006, 0.006, 1.] }
            else if depth.is_float() { [2., -0.125, 0.25, alpha] }
            else { [0.4, 0.03, 0.1, alpha] };
        for engine in [&mut live, &mut full] {
            engine.set_brush(brush.clone()).unwrap();
            engine.set_instant_feedback(layer_engine::InstantFeedbackConfig {
                enabled: true, prediction_horizon_micros: 16_000, ..Default::default()
            }).unwrap();
        }
        for stroke in 0..2 {
            for frame in 0..10 {
                // Alternate sample counts, revisit saturated pixels, vary
                // pressure and cross a page boundary in the same stroke.
                let count = if frame % 2 == 0 { 1 } else { 3 };
                let now = 1_000_000_000 + (stroke * 10 + frame + 1) * 33_333_333;
                for sample in 0..count {
                    let event = PenEvent {
                        device_id: 1, sequence: now + sample,
                        timestamp_ns: now - (count - sample - 1) * 5_000_000,
                        view_revision: 0,
                        surface_position: layer_core::Point {
                            x: if large { 1300. + frame as f32 * 150. + sample as f32 * 25. }
                                else { 170. + frame as f32 * 9. + sample as f32 * 2. },
                            y: if large { 1536. + (frame as f32 * 0.7).sin() * 250. }
                                else { 175. + (frame as f32 * 0.7).sin() * 12. },
                        },
                        pressure: if frame == 4 { 0.4 } else { 1. },
                        tilt_radians: [0.; 2], twist_radians: 0., distance: 0.,
                        phase: if frame == 0 { PenPhase::Down }
                            else if frame == 9 && sample == count - 1 { PenPhase::Up }
                            else { PenPhase::Move },
                        tool: ToolKind::Pen, flags: SampleFlags::PRIMARY,
                    };
                    input.push(event).unwrap();
                    full_input.push(event).unwrap();
                }
                live.render_frame_for(now, now + 16_000_000).unwrap();
                full.render_frame_for(now, now + 16_000_000).unwrap();
                // Large contacts are compared through exact native backing
                // below, avoiding thousands of per-page synchronous readbacks.
                if large { continue; }
                let actual = state(live.backend());
                let expected = state(full.backend());
                assert_eq!(actual.len(), expected.len());
                for (a, b) in actual.iter().zip(&expected) {
                    assert_eq!(a.0, b.0);
                    assert!(a.1 == b.1, "working/coverage/preview: {depth:?} stroke={stroke} frame={frame} page={:?}", a.0);
                }
            }
            flush(&mut live);
            flush(&mut full);
            assert!(backing(&live.document().layers[0].raster) == backing(&full.document().layers[0].raster),
                "native publication: {depth:?} stroke={stroke}");
        }
    }
}

#[test]
fn native_gpen_batch_edges_do_not_darken_opaque_source_pixels() {
    use layer_core::color::{ColorProfile, source::*};
    // Prediction leaves finalized contacts at different offsets/batch sizes.
    // This exposed dark crescents in both fragment and compute specialization
    // on Adreno; a small brush or prediction-disabled stroke missed the defect.
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        for fragment in [false, true] {
            let extent = [4097, 2049];
            let mut document = layer_core::Document::new("G-Pen batch edges", extent[0], extent[1]);
            document.color.depth = depth;
            document.layers[1].visible = false;
            let mut source = SourceBuilder::new(
                extent,
                SourceInterpretation {
                    channels: SourceChannels::Rgb,
                    depth: SampleDepth::U8,
                    profile: ColorProfile::Builtin(RgbSpace::Srgb),
                    profile_assumed: false,
                },
                64 * 1024 * 1024,
            )
            .unwrap();
            let row = [230, 230, 230].repeat(extent[0] as usize);
            for _ in 0..extent[1] {
                source.push_row(&row).unwrap();
            }
            document.layers[0].source = Some(Arc::new(source.finish().unwrap()));
            let (mut input, mut live) = engine(document);
            if fragment {
                live.backend_mut().pipelines.dry_material = None;
            }
            let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
            brush.diameter = 2048.;
            brush.color_rgba_linear = [
                0.5,
                0.6,
                0.05,
                if depth == SampleDepth::U8 { 1. } else { 0.35 },
            ];
            live.set_brush(brush).unwrap();
            live.set_instant_feedback(layer_engine::InstantFeedbackConfig {
                enabled: true,
                prediction_horizon_micros: 16_000,
                ..Default::default()
            })
            .unwrap();
            for i in 0..18 {
                input
                    .push(PenEvent {
                        device_id: 1,
                        sequence: i + 1,
                        timestamp_ns: (i + 1) * 16_666_667,
                        view_revision: 0,
                        surface_position: layer_core::Point {
                            x: 650. + i as f32 * 140.,
                            y: 1020. + (i as f32 * 0.2).sin() * 230.,
                        },
                        pressure: 0.35 + 0.6 * (i as f32 / 17.),
                        tilt_radians: [0.; 2],
                        twist_radians: 0.,
                        distance: 0.,
                        phase: if i == 0 {
                            PenPhase::Down
                        } else if i == 17 {
                            PenPhase::Up
                        } else {
                            PenPhase::Move
                        },
                        tool: ToolKind::Pen,
                        flags: SampleFlags::PRIMARY,
                    })
                    .unwrap();
                live.render_frame_for((i + 1) * 16_666_667, (i + 1) * 16_666_667 + 16_000_000)
                    .unwrap();
            }
            flush(&mut live);
            let stored = backing(&live.document().layers[0].raster);
            assert!(!stored.is_empty());
            let stride = usize::from(depth.bits() / 8);
            let maximum = if depth == SampleDepth::U8 { 255 } else { 65535 };
            let background = 230 * (maximum / 255);
            // Normal source-over stays between the ink and opaque background.
            // Allow two integer code values for transfer/quantization rounding.
            let minimum = [0.5_f64, 0.6, 0.05].map(|linear| {
                ((1.055 * linear.powf(1. / 2.4) - 0.055) * f64::from(maximum)).floor() as u32 - 2
            });
            let mut dark = 0;
            let mut first = None;
            let mut painted = 0;
            for (key, tile) in &stored {
                if key.plane != RasterPlane::Color {
                    continue;
                }
                for (index, pixel) in tile.chunks_exact(4 * stride).enumerate() {
                    let point = [
                        key.coordinate[0] * PAGE_SIZE + index as u32 % PAGE_SIZE,
                        key.coordinate[1] * PAGE_SIZE + index as u32 / PAGE_SIZE,
                    ];
                    if point[0] >= extent[0] || point[1] >= extent[1] {
                        continue;
                    }
                    let rgba: [u32; 4] = std::array::from_fn(|channel| {
                        let offset = channel * stride;
                        if stride == 1 {
                            u32::from(pixel[offset])
                        } else {
                            u32::from(u16::from_le_bytes([pixel[offset], pixel[offset + 1]]))
                        }
                    });
                    assert_eq!(
                        rgba[3], maximum,
                        "photo alpha at {point:?}, {depth:?}, fragment={fragment}"
                    );
                    assert!(rgba[..3].iter().all(|v| *v <= background + 2));
                    painted += usize::from(rgba[0] + 2 < background);
                    if (0..3).any(|channel| rgba[channel] < minimum[channel]) {
                        dark += 1;
                        first.get_or_insert((point, rgba));
                    }
                }
            }
            assert!(painted > 1000, "stroke must actually paint the source");
            assert_eq!(
                dark, 0,
                "G-Pen darkened {dark} source pixels; first={first:?}, {depth:?}, fragment={fragment}"
            );
        }
    }
}

#[test]
fn native_gpen_keeps_original_photo_pixels_in_touched_tiles() {
    use layer_core::color::{ColorProfile, source::*};
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        for in_place in [false, true] {
            let mut document = layer_core::Document::new("photo pen", 4353, 769);
            document.color = DocumentColor {
                space: RgbSpace::Srgb,
                depth,
            };
            document.layers[1].visible = false;
            let mut source = SourceBuilder::new(
                [4353, 769],
                SourceInterpretation {
                    channels: SourceChannels::Rgb,
                    depth: SampleDepth::U8,
                    profile: ColorProfile::Builtin(RgbSpace::Srgb),
                    profile_assumed: false,
                },
                32 * 1024 * 1024,
            )
            .unwrap();
            for _ in 0..769 {
                source.push_row(&[70, 140, 210].repeat(4353)).unwrap();
            }
            document.layers[0].source = Some(Arc::new(source.finish().unwrap()));
            let (mut input, mut live) = engine(document);
            let r = live.backend_mut();
            let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
            r.native_edit = Some(NativeEdit::with_mode(r, transfer, in_place));
            let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
            brush.color_rgba_linear = [1., 0., 0.7, 1. / 3.];
            live.set_brush(brush).unwrap();
            stroke(&mut live, &mut input, 1, 60.);
            let stored = backing(&live.document().layers[0].raster);
            let tile = &stored[&TileKey {
                plane: RasterPlane::Color,
                coordinate: [0, 0],
            }];
            let stride = usize::from(depth.bits() / 8) * 4;
            let expected: Vec<u8> = match depth {
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only fixture"),
                SampleDepth::U8 => vec![70, 140, 210, 255],
                SampleDepth::U16 => [70u16, 140, 210, 255]
                    .into_iter()
                    .flat_map(|v| (v * 257).to_le_bytes())
                    .collect(),
            };
            assert_eq!(
                &tile[..stride],
                expected,
                "untouched corner {depth:?} in_place={in_place}"
            );
            for (i, pixel) in tile.chunks_exact(stride).enumerate() {
                assert!(
                    pixel[stride * 3 / 4..].iter().all(|v| *v == 255),
                    "photo alpha lost at pixel {i}: {depth:?} in_place={in_place}"
                );
            }
        }
    }
}

#[test]
#[ignore = "60 MP GPU publication, exact save/history and device replacement; run alone"]
fn native_60mp_source_transform_commits_and_preserves_history() {
    native_large_source_transform(false);
}

#[test]
#[ignore = "60 MP linked color/mask publication; run alone"]
fn native_60mp_linked_mask_transform_commits_and_preserves_history() {
    native_large_source_transform(true);
}

fn native_large_source_transform(linked_mask: bool) {
    use layer_core::color::{ColorProfile, source::*};
    use layer_core::{Affine, ImageTransform, LayerMask, LayerOperation, LayerOperationKind, Point};
    let extent = [8192, 7324];
    let color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
    let mut document = layer_core::Document::new("60 MP native transform", extent[0], extent[1]);
    document.color = color;
    let mut source = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: color.depth,
        profile: ColorProfile::Builtin(color.space),
        profile_assumed: false,
    }, 512 * 1024 * 1024).unwrap();
    let row: Vec<_> = (0..extent[0]).flat_map(|x|
        [16000 + x as u16, 32000, 48000, 65535].into_iter().flat_map(u16::to_le_bytes)
    ).collect();
    for _ in 0..extent[1] { source.push_row(&row).unwrap(); }
    let source = Arc::new(source.finish().unwrap());
    document.layers[0].source = Some(source.clone());
    if linked_mask {
        let id = document.allocate_layer_id();
        let mut mask = LayerMask::reveal_all(id, Point::default());
        mask.linked = true;
        mask.default_coverage = 0.;
        let tile = RasterTile::backed(TileBlob::encode(color.coverage_descriptor(), &32768_u16.to_le_bytes().repeat(65536)).unwrap());
        mask.raster = RasterRevision::backed(RasterData {
            tiles: (0..29).flat_map(|y| (0..32).map(move |x| [x, y]))
                .map(|coordinate| (TileKey { plane: RasterPlane::Mask, coordinate }, tile.clone()))
                .collect(),
            ..Default::default()
        });
        document.layers[0].mask = Some(mask);
    }
    let id = document.layers[0].id;
    let original = document.layers[0].raster.clone();
    let original_mask = document.layers[0].mask.as_ref().map(|mask| mask.raster.clone());
    let (_, mut live) = engine(document);
    let transform = ImageTransform {
        affine: Affine::translation(Point { x: 256., y: 0. }),
        ..Default::default()
    };
    if linked_mask {
        live.set_transform_preview(Some(layer_render::TransformPreview {
            transaction: 1, layer: id, selection: None, transform,
        })).unwrap();
        flush(&mut live);
        assert!(live.commit_transform().unwrap());
    } else {
        let mask_id = live.allocate_layer_id();
        live.append_layer_operation(id, LayerOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage: LayerMask::reveal_all(mask_id, Point::default()),
            kind: LayerOperationKind::Transform(transform),
        }).unwrap();
    }
    flush(&mut live);
    let edited = live.document().layers[0].raster.clone();
    // Compare stored digests, not a second full decoded image allocation.
    let digests = |root: &RasterRevision| -> BTreeMap<_, _> {
        root.wait_data().unwrap().tiles.iter().map(|(key, tile)|
            (*key, tile.wait_backing().unwrap().digest)
        ).collect()
    };
    let committed = digests(&edited);
    let committed_mask = live.document().layers[0].mask.as_ref().map(|mask| digests(&mask.raster));
    assert_eq!(committed.len(), 32 * 29);
    assert_eq!(live.document().layers[0].source.as_ref().unwrap(), &source);
    let data = edited.wait_data().unwrap();
    // Integer translation keeps opaque source samples exact, including the
    // final partial tile row. Exposed original pixels are overridden by zero.
    for (coordinate, local, expected) in [
        ([0, 0], [0, 0], [0_u16; 4]),
        ([1, 0], [0, 0], [16000, 32000, 48000, 65535]),
        ([31, 28], [255, 155], [23935, 32000, 48000, 65535]),
    ] {
        let bytes = data.tiles[&TileKey { plane: RasterPlane::Color, coordinate }]
            .wait_backing().unwrap().decode().unwrap();
        let index = (local[1] * 256 + local[0]) * 8;
        let actual: Vec<_> = bytes[index..index + 8].chunks_exact(2)
            .map(|v| u16::from_le_bytes(v.try_into().unwrap())).collect();
        assert_eq!(actual, expected, "tile {coordinate:?} pixel {local:?}");
    }
    if linked_mask {
        let mask = live.document().layers[0].mask.as_ref().unwrap().raster.wait_data().unwrap();
        for (coordinate, expected) in [([0, 0], 0), ([1, 0], 32768), ([31, 28], 32768)] {
            let bytes = mask.tiles[&TileKey { plane: RasterPlane::Mask, coordinate }]
                .wait_backing().unwrap().decode().unwrap();
            assert_eq!(u16::from_le_bytes(bytes[..2].try_into().unwrap()), expected);
        }
    }
    assert!(live.undo().unwrap());
    flush(&mut live);
    assert_eq!(digests(&live.document().layers[0].raster), digests(&original));
    assert_eq!(live.document().layers[0].mask.as_ref().map(|mask| digests(&mask.raster)), original_mask.as_ref().map(digests));
    assert!(live.redo().unwrap());
    flush(&mut live);
    assert_eq!(digests(&live.document().layers[0].raster), committed);
    assert_eq!(live.document().layers[0].mask.as_ref().map(|mask| digests(&mask.raster)), committed_mask);
    let project = layer_core::Project { document: live.document().clone(), assets: Default::default() };
    let mut bytes = Vec::new();
    project.write(&mut bytes).unwrap();
    let loaded = layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap();
    assert_eq!(digests(&loaded.document.layers[0].raster), committed);
    assert_eq!(loaded.document.layers[0].mask.as_ref().map(|mask| digests(&mask.raster)), committed_mask);
    let replacement = WgpuRasterizer::new_native_headless(color).unwrap();
    drop(live.replace_backend(replacement).unwrap());
    flush(&mut live);
    assert_eq!(digests(&live.document().layers[0].raster), committed);
    assert_eq!(live.document().layers[0].mask.as_ref().map(|mask| digests(&mask.raster)), committed_mask);
}

fn packet<'a>(layers: &'a [Layer], reset: bool) -> FramePacket<'a> {
    FramePacket {
        view: view(),
        document_extent: [256 * 17, 256],
        layers,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: reset,
        time_seconds: 0.,
        composite_all: true,
    }
}
fn restored_fixture(r: &mut WgpuRasterizer) -> Vec<Layer> {
    let color = r.document_color();
    let mut layers = vec![Layer::paint(LayerId(1), "many tiles")];
    let mut data = RasterData::default();
    for plane in [
        RasterPlane::Color,
        RasterPlane::Wetness,
        RasterPlane::WatercolorWetness,
    ] {
        for x in 0..17 {
            let bytes: Vec<_> = (0..65536u32)
                .flat_map(|pixel| {
                    (0..plane.descriptor(color).channels).flat_map(move |channel| {
                        let value = if channel == 3 {
                            17
                        } else {
                            pixel.wrapping_mul(11 + x + channel as u32)
                        };
                        (value as u16)
                            .to_le_bytes()
                            .into_iter()
                            .take(color.depth.bytes())
                    })
                })
                .collect();
            data.tiles.insert(
                TileKey {
                    plane,
                    coordinate: [x, 0],
                },
                RasterTile::backed(TileBlob::encode(plane.descriptor(color), &bytes).unwrap()),
            );
        }
    }
    let mask_data = RasterData {
        tiles: data
            .tiles
            .iter()
            .filter(|(key, _)| key.plane == RasterPlane::Wetness)
            .map(|(key, tile)| {
                (
                    TileKey {
                        plane: RasterPlane::Mask,
                        coordinate: key.coordinate,
                    },
                    tile.clone(),
                )
            })
            .collect(),
        watercolor: None,
    };
    let mut mask = layer_core::LayerMask::reveal_all(LayerId(2), Default::default());
    mask.raster = RasterRevision::backed(mask_data);
    layers[0].mask = Some(mask);
    layers[0].raster = RasterRevision::backed(data);
    r.submit(packet(&layers, true)).unwrap();
    layers
}
fn mark_changed(r: &mut WgpuRasterizer, layers: &mut [Layer]) {
    layers[0].raster = RasterRevision::pending();
    layers[0].mask.as_mut().unwrap().raster = RasterRevision::pending();
    for id in [LayerId(1), LayerId(2)] {
        r.raster
            .as_mut()
            .unwrap()
            .targets
            .get_mut(&id)
            .unwrap()
            .changed
            .extend((0..17).map(|x| [x, 0]));
    }
}

#[test]
fn native_commit_reuses_scratch_across_color_and_coverage_chunks_without_changing_codes() {
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let mut layers = restored_fixture(&mut r);
    let before = backing(&layers[0].raster);
    let mask_before = backing(&layers[0].mask.as_ref().unwrap().raster);
    let scratch = r.native_edit.as_ref().unwrap().storage_bytes();
    for _ in 0..3 {
        mark_changed(&mut r, &mut layers);
        while !r.raster_ready() {
            std::thread::yield_now();
        }
        r.submit(packet(&layers, false)).unwrap();
        assert_eq!(backing(&layers[0].raster), before);
        assert_eq!(
            backing(&layers[0].mask.as_ref().unwrap().raster),
            mask_before
        );
        assert_eq!(r.native_edit.as_ref().unwrap().storage_bytes(), scratch);
    }
    // An unchanged pending revision reuses every backing ticket, with no copies.
    let previous = layers[0].raster.wait_data().unwrap();
    layers[0].raster = RasterRevision::pending();
    while !r.raster_ready() {
        std::thread::yield_now();
    }
    r.submit(packet(&layers, false)).unwrap();
    let current = layers[0].raster.wait_data().unwrap();
    assert!(
        current
            .tiles
            .iter()
            .all(|(key, tile)| tile.same_capture(&previous.tiles[key]))
    );
}

fn set_pixel(r: &WgpuRasterizer, texture: &wgpu::Texture, values: &[f32]) {
    r.queue.write_texture(
        texture.as_image_copy(),
        &values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: None,
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
}

#[test]
fn invalid_late_color_or_mask_rejects_every_chunk_without_partial_canonical_adoption() {
    for mask_failure in [false, true] {
        let color = DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: SampleDepth::U16,
        };
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut layers = restored_fixture(&mut r);
        let checkpoint = layers.clone();
        let first = r.paint_layers[0]
            .pages
            .iter()
            .find(|p| p.coordinate == [0, 0])
            .unwrap()
            .active()
            .texture
            .clone();
        // A valid value deliberately between native codes exposes an early
        // promotion even though a later page rejects the whole publication.
        set_pixel(&r, &first, &[0.01234567, 0.2345678, 0.7890123, 1.]);
        let bad = if mask_failure {
            r.layer_masks.pages[&(LayerId(2), [16, 0])].texture.clone()
        } else {
            r.paint_layers[0]
                .pages
                .iter()
                .find(|p| p.coordinate == [16, 0])
                .unwrap()
                .active()
                .texture
                .clone()
        };
        set_pixel(
            &r,
            &bad,
            if mask_failure {
                &[-0.25]
            } else {
                &[f32::NAN, 0., 0., 1.]
            },
        );
        let before = working(&r, LayerId(1));
        let mask_before = working(&r, LayerId(2));
        mark_changed(&mut r, &mut layers);
        while !r.raster_ready() {
            std::thread::yield_now();
        }
        r.submit(packet(&layers, false)).unwrap();
        for root in [&layers[0].raster, &layers[0].mask.as_ref().unwrap().raster] {
            for tile in root.wait_data().unwrap().tiles.values() {
                assert!(tile.wait_backing().is_err(), "all capture chunks must fail");
            }
            assert!(!root.host_backed());
        }
        assert_eq!(working(&r, LayerId(1)), before);
        assert_eq!(working(&r, LayerId(2)), mask_before);
        // The last host-backed checkpoint is independent of the failed frame.
        assert!(checkpoint[0].raster.host_backed());
        assert!(checkpoint[0].mask.as_ref().unwrap().raster.host_backed());
        let mut replacement = WgpuRasterizer::new_native_headless(color).unwrap();
        replacement.submit(packet(&checkpoint, true)).unwrap();
        assert_eq!(
            backing(&checkpoint[0].raster),
            backing(&replacement.raster.as_ref().unwrap().targets[&LayerId(1)].revision)
        );
    }
}

#[test]
fn abandoned_native_frame_fails_roots_and_tile_waiters_without_submitting_edits() {
    let color = DocumentColor {
        space: RgbSpace::AdobeRgb,
        depth: SampleDepth::U16,
    };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let mut layers = restored_fixture(&mut r);
    let before = working(&r, LayerId(1));
    mark_changed(&mut r, &mut layers);
    while !r.raster_ready() {
        std::thread::yield_now();
    }
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    let frame = r
        .encode_native_rasters(&layers, &mut encoder)
        .unwrap()
        .unwrap();
    let tickets: Vec<_> = frame
        .publications
        .iter()
        .flat_map(|p| p.data.tiles.values().cloned())
        .collect();
    drop(frame);
    drop(encoder);
    assert!(layers[0].raster.wait_data().is_err());
    assert!(layers[0].mask.as_ref().unwrap().raster.wait_data().is_err());
    assert!(
        tickets
            .iter()
            .all(|tile| matches!(tile.try_backing(), Some(Err(_))))
    );
    assert_eq!(working(&r, LayerId(1)), before);
}

#[test]
fn srgb8_codes_and_coverage_are_independent_through_native_publication() {
    use layer_core::color::{AlphaAssociation, TransferEncoding};
    let color = DocumentColor::default();
    let descriptor = color.paint_descriptor();
    assert_eq!(descriptor.alpha, AlphaAssociation::Straight);
    assert_eq!(descriptor.encoding, TransferEncoding::Profile);
    // Every 8-bit RGB code at every nonzero alpha. Paint canonicalizes unused
    // alpha-zero RGB, while retained image sources preserve their hidden codes.
    let bytes: Vec<u8> = (0..256u32)
        .flat_map(|a| {
            (0..256u32).flat_map(move |x| {
                if a == 0 {
                    [0; 4]
                } else {
                    [x as u8, (255 - x) as u8, (x * 71) as u8, a as u8]
                }
            })
        })
        .collect();
    let key = TileKey {
        plane: RasterPlane::Color,
        coordinate: [0, 0],
    };
    let mut layer = Layer::paint(LayerId(1), "sRGB code/coverage grid");
    layer.raster = RasterRevision::backed(RasterData {
        tiles: [(
            key,
            RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()),
        )]
        .into(),
        watercolor: None,
    });
    let mut layers = [layer];
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let submit = |r: &mut WgpuRasterizer, layers: &[Layer], reset| {
        r.submit(FramePacket {
            document_extent: [256, 256],
            ..packet(layers, reset)
        })
        .unwrap();
    };
    submit(&mut r, &layers, true);
    let samples = working(&r, LayerId(1));
    for (i, (encoded, actual)) in bytes
        .chunks_exact(4)
        .zip(samples[&key].chunks_exact(16))
        .enumerate()
    {
        let alpha = encoded[3] as f64 / 255.;
        for c in 0..4 {
            let v = encoded[c] as f64 / 255.;
            let expected = if c == 3 {
                alpha
            } else {
                let linear = if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                };
                linear * alpha
            };
            let actual = f32::from_ne_bytes(actual[c * 4..c * 4 + 4].try_into().unwrap()) as f64;
            assert!(
                (actual - expected).abs() < 2e-7,
                "pixel {i} channel {c}: {actual} vs {expected}"
            );
        }
    }
    for _ in 0..16 {
        layers[0].raster = RasterRevision::pending();
        r.raster
            .as_mut()
            .unwrap()
            .targets
            .get_mut(&LayerId(1))
            .unwrap()
            .changed
            .insert([0, 0]);
        while !r.raster_ready() {
            std::thread::yield_now();
        }
        submit(&mut r, &layers, false);
        assert_eq!(backing(&layers[0].raster)[&key], bytes);
    }
}

#[test]
fn batched_validation_scans_every_texture_slot_and_partial_tail() {
    let r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    }).unwrap();
    let native = r.native_edit.as_ref().unwrap();
    let textures: Vec<_> = [wgpu::TextureFormat::Rgba32Float, wgpu::TextureFormat::R32Float]
        .into_iter().flat_map(|format| (0..17).map(move |_| format))
        .map(|format| r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("validation slot fixture"),
            size: wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })).collect();
    let inputs: Vec<_> = textures.iter().map(|texture| {
        let plane = if texture.format() == wgpu::TextureFormat::Rgba32Float {
            RasterPlane::Color
        } else { RasterPlane::Mask };
        (texture, RasterTile::pending(plane.descriptor(r.document_color())))
    }).collect();
    let readback = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("validation status fixture"), size: STATUS_BYTES,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let status = NativeEncodeStatus::new(&r.device);
    let scan = || {
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        status.reset(&mut encoder);
        native.validator.encode(&r, &mut encoder, &inputs, &status, &mut Default::default()).unwrap();
        encoder.copy_buffer_to_buffer(status.buffer(), 0, &readback, 0, STATUS_BYTES);
        let submission = encoder.submit(&r.queue);
        let (tx, rx) = mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| { tx.send(result).unwrap(); });
        r.device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(READBACK_TIMEOUT) }).unwrap();
        rx.recv_timeout(READBACK_TIMEOUT).unwrap().unwrap();
        let result = NativeEncodeStatus::decode(&readback.get_mapped_range(..).unwrap());
        readback.unmap();
        result
    };
    assert!(scan().is_ok());
    for (slot, texture) in textures.iter().enumerate() {
        let color = texture.format() == wgpu::TextureFormat::Rgba32Float;
        // The last invocation of each independently bound image must contribute
        // to global failure, including both groups' one-tile trailing batches.
        let write = |value: f32| r.queue.write_texture(
            wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x: 255, y: 255, z: 0 }, ..texture.as_image_copy() },
            &[value, 0., 0., 1.][..if color { 4 } else { 1 }].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: None, rows_per_image: None },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        write(f32::NAN);
        assert!(scan().is_err(), "invalid slot {slot} was not visited");
        write(0.);
    }
    assert!(scan().is_ok());
}

#[test]
fn deferred_native_outputs_preserve_versions_and_status_during_following_frames() {
    let color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let mut layers = restored_fixture(&mut r);
    let before = backing(&layers[0].raster);
    let mask_before = backing(&layers[0].mask.as_ref().unwrap().raster);
    while !r.raster_ready() { std::thread::yield_now(); }
    let presentation = r.prioritize_raster_presentation();
    mark_changed(&mut r, &mut layers);
    r.submit(packet(&layers, false)).unwrap();
    let first = layers.clone();
    let page = r.paint_layers[0].pages.iter().find(|p| p.coordinate == [0, 0]).unwrap().active().texture.clone();
    set_pixel(&r, &page, &[0.; 4]);
    let mask = r.layer_masks.pages[&(LayerId(2), [0, 0])].texture.clone();
    set_pixel(&r, &mask, &[0.5]);
    mark_changed(&mut r, &mut layers);
    r.submit(packet(&layers, false)).unwrap();
    let second = layers.clone();
    // A later failed publication must not poison either earlier status buffer.
    set_pixel(&r, &page, &[f32::NAN, 0., 0., 1.]);
    mark_changed(&mut r, &mut layers);
    r.submit(packet(&layers, false)).unwrap();
    r.device.poll(wgpu::PollType::Wait { submission_index: r.last_submission.clone(), timeout: Some(READBACK_TIMEOUT) }).unwrap();
    for snapshot in [&first, &second, &layers] {
        for root in [&snapshot[0].raster, &snapshot[0].mask.as_ref().unwrap().raster] {
            assert!(root.wait_data().unwrap().tiles.values().all(|t| t.try_backing().is_none()));
        }
    }
    assert_eq!(r.raster_buffers.transfer.load(Ordering::Relaxed), 0);
    drop(presentation);
    assert_eq!(backing(&first[0].raster), before);
    assert_eq!(backing(&first[0].mask.as_ref().unwrap().raster), mask_before);
    let mut expected = before;
    expected.get_mut(&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }).unwrap()[..8].fill(0);
    let mut mask_expected = mask_before;
    mask_expected.get_mut(&TileKey { plane: RasterPlane::Mask, coordinate: [0, 0] }).unwrap()[..2].copy_from_slice(&32768u16.to_le_bytes());
    assert_eq!(backing(&second[0].raster), expected);
    assert_eq!(backing(&second[0].mask.as_ref().unwrap().raster), mask_expected);
    for root in [&layers[0].raster, &layers[0].mask.as_ref().unwrap().raster] {
        assert!(root.wait_data().unwrap().tiles.values().all(|t| t.wait_backing().is_err()));
    }
}

#[test]
fn pending_native_save_and_immediate_undo_finish_after_presentation_releases_backing() {
    let mut document = layer_core::Document::new("pending backing", 256, 256);
    document.color = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 };
    let (mut input, mut live) = engine(document);
    while !live.backend().raster_ready() { std::thread::yield_now(); }
    let presentation = live.backend().prioritize_raster_presentation();
    stroke(&mut live, &mut input, 1, 60.);
    let first = live.document().layers[0].raster.clone();
    stroke(&mut live, &mut input, 10, 75.);
    let second = live.document().layers[0].raster.clone();
    assert!(!first.host_backed());
    assert!(!second.host_backed());
    let project = layer_core::Project { document: live.document().clone(), assets: Default::default() };
    let (started, ready) = mpsc::channel();
    let save = std::thread::spawn(move || {
        started.send(()).unwrap();
        let mut bytes = Vec::new();
        project.write(&mut bytes).unwrap();
        layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap()
    });
    ready.recv_timeout(READBACK_TIMEOUT).unwrap();
    assert!(live.undo().unwrap());
    // Restore cannot yet read the pending first version. It yields without
    // blocking the renderer or replacing the immutable save snapshot.
    live.render_frame().unwrap();
    drop(presentation);
    flush(&mut live);
    let first_bytes = backing(&first);
    let second_bytes = backing(&second);
    assert_ne!(first_bytes, second_bytes);
    assert_eq!(backing(&live.document().layers[0].raster), first_bytes);
    let saved = save.join().unwrap();
    assert_eq!(backing(&saved.document.layers[0].raster), second_bytes);
    assert!(live.redo().unwrap());
    flush(&mut live);
    assert_eq!(backing(&live.document().layers[0].raster), second_bytes);
}

#[test]
fn device_loss_before_deferred_native_backing_keeps_the_last_checkpoint() {
    let color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let mut layers = restored_fixture(&mut r);
    let checkpoint = layers.clone();
    let expected = backing(&checkpoint[0].raster);
    while !r.raster_ready() { std::thread::yield_now(); }
    let presentation = r.prioritize_raster_presentation();
    mark_changed(&mut r, &mut layers);
    r.submit(packet(&layers, false)).unwrap();
    r.device.poll(wgpu::PollType::Wait { submission_index: r.last_submission.clone(), timeout: Some(READBACK_TIMEOUT) }).unwrap();
    assert!(!layers[0].raster.host_backed());
    r.device.destroy();
    drop(presentation);
    for root in [&layers[0].raster, &layers[0].mask.as_ref().unwrap().raster] {
        assert!(root.wait_data().unwrap().tiles.values().all(|tile| tile.wait_backing().is_err()));
    }
    assert!(checkpoint[0].raster.host_backed());
    let mut replacement = WgpuRasterizer::new_native_headless(color).unwrap();
    replacement.submit(packet(&checkpoint, true)).unwrap();
    assert_eq!(backing(&checkpoint[0].raster), expected);
}

#[test]
fn native_in_place_runtime_matches_candidate_fallback_for_mixed_planes() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let color = DocumentColor { space: RgbSpace::ProPhoto, depth };
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        assert!(r.native_edit.as_ref().unwrap().promoter.is_none(), "in-place feature must be active for this comparison");
        let mut layers = restored_fixture(&mut r);
        let expected = backing(&layers[0].raster);
        let mask_expected = backing(&layers[0].mask.as_ref().unwrap().raster);
        for in_place in [false, true] {
            let transfer = r.prepare_native_transfer(color.space).unwrap();
            r.native_edit = Some(NativeEdit::with_mode(&r, transfer, in_place));
            mark_changed(&mut r, &mut layers);
            while !r.raster_ready() { std::thread::yield_now(); }
            r.submit(packet(&layers, false)).unwrap();
            assert_eq!(backing(&layers[0].raster), expected);
            assert_eq!(backing(&layers[0].mask.as_ref().unwrap().raster), mask_expected);
        }
    }
}
