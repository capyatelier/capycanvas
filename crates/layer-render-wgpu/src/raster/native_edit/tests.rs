use super::*;
use layer_core::{Document, authored::PortableId};
use crate::layer_tests::placement::{paint_document, target, paint, paint_mut, occurrence_id, set_mask, reveal_all};

fn mask_target(document: &Document) -> SourceTarget {
    SourceTarget::Coverage(document.scene().occurrence(occurrence_id(document)).unwrap().mask.as_ref().unwrap().source)
}
fn mask_raster(document: &Document) -> &RasterRevision {
    document.target_raster(mask_target(document)).unwrap()
}
fn set_color(document: &mut Document, color: DocumentColor) {
    let root = document.artwork.root;
    document.artwork.compositions.get_mut(root).unwrap().color = color;
}
fn write_capture(capture: &layer_core::ArtworkCapture, bytes: &mut Vec<u8>) -> Result<(),String> {
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    layer_core::package::codec::PreparedPackage::prepare(capture,None,&cancelled)?.write(bytes,&cancelled)
}
fn read_document(bytes: Vec<u8>) -> Document {
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let source = layer_core::package::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
    let layer_core::package::codec::OpenOutcome::Candidate {artwork,..} =
        layer_core::package::codec::open(source,Default::default(),&cancelled).unwrap() else {panic!("Expected editable native raster package")};
    let document = Document::from_artwork(artwork).unwrap();
    document.validate(Default::default()).unwrap();
    document
}
use layer_core::color::{SampleDepth, RgbSpace};
use layer_engine::{
    CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ViewTransform,
    input_queue,
};
use layer_render::ViewState;

fn view() -> ViewState {
    ViewState {
        width_px: 256,
        height_px: 256,
        document_to_surface: [1., 0., 0., 1., 0., 0.],
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
            .push(PenEvent { timestamp_ns: (sequence + i as u64) * 10_000_000, pressure: 0.37,
                    ..crate::test_support::pen(sequence + i as u64, phase, [x + i as f32 * 9., 80.], SampleFlags::PRIMARY) })
            .unwrap();
        flush(engine);
    }
}
fn working(r: &WgpuRasterizer, id: SourceTarget) -> BTreeMap<TileKey, Vec<u8>> {
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
    mut document: layer_core::Document,
) -> (InputProducer<PenEvent>, CanvasEngine<WgpuRasterizer>) {
    if document.active_target().is_none() {
        document.working.occurrence = Some(occurrence_id(&document));
        document.working.target = Some(target(&document));
    }
    let r = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
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
fn partial_bakes_back_tiles_without_publishing_the_layer() {
    let mut document = layer_core::Document::new(PortableId::random(), 1280, 768, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let root = document.artwork.root;
    document.artwork.compositions.get_mut(root).unwrap().blend = layer_core::BlendSpace::Perceptual;
    paint_mut(&mut document).original = Some(layer_core::color::source::rgba8_source([1280, 768], |x, y| {
        [(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255]
    }));
    let photo = occurrence_id(&document);
    let (_, mut engine) = engine(document);
    let pixels = |engine: &mut CanvasEngine<WgpuRasterizer>| {
        engine.backend_mut().readback_srgb_rgba8().unwrap()
    };
    let original = pixels(&mut engine);
    engine.backend_mut().native_edit.as_mut().unwrap().color_cache_bytes = 4 << 20;
    let filters = layer_core::SeparationFilters::new(layer_core::bundled_effect_catalog(), 8.).unwrap();
    let plan = engine.document().separation_plan(photo, &filters, ["Frequency Separation", "Low", "High"].map(std::sync::Arc::from)).unwrap();
    let low = plan.operations[0].0;
    engine.insert_with_operations(plan.edits, plan.operations, None).unwrap();
    engine.render_frame().unwrap();
    let root = engine.document().target_raster(low).unwrap().clone();
    assert!(root.try_data().is_none());
    let r = engine.backend();
    let data = &r.native_edit.as_ref().unwrap().backing[&low];
    assert!(!data.tiles.is_empty() && data.tiles.len() < 15, "the first region has backing while the layer is incomplete");
    let (&key, tile) = data.tiles.iter().next().unwrap();
    let first = tile.wait_backing().unwrap().decode().unwrap();
    flush(&mut engine);
    let data = root.wait_data().unwrap();
    assert_eq!(data.tiles.len(), 15);
    assert_eq!(data.tiles[&key].wait_backing().unwrap().decode().unwrap(), first);
    let actual = pixels(&mut engine);
    assert!(actual.iter().zip(&original).all(|(a, b)| a.abs_diff(*b) <= 1), "High must use Low after its working pages are evicted");
    assert!(engine.undo().unwrap());
    flush(&mut engine);
    assert!(engine.redo().unwrap());
    flush(&mut engine);
    assert_eq!(engine.document().target_raster(low).unwrap().wait_data().unwrap().tiles.len(), 15);
    assert_eq!(pixels(&mut engine), actual);
}

#[test]
fn native_extended_fill_gradient_and_figure_pixels_survive_history_and_save() {
    use layer_core::{Affine, Figure, FigurePaint, FigureShape, RasterOperation, RasterOperationKind, Point};
    use layer_core::color::{RgbColor, f16};
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16] {
            let color = DocumentColor { space, depth };
            let mut document = layer_core::Document::new(PortableId::random(), 384, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
            set_color(&mut document,color);
            let id = target(&document);
            let (_, mut live) = engine(document);
            let empty = backing(&paint(live.document()).raster);
            let colors = if depth.is_float() {
                [[8., -0.125, 2., 0.625], [-0.25, 3., 0.5, 0.375]]
            } else {
                [[1., 0., 0., 0.625], [0., 1., 0.5, 0.375]].map(|p|
                    RgbColor::new(RgbSpace::DisplayP3, p).unwrap().linear_in(space).unwrap())
            };
            let start = Point { x: 64., y: 32. };
            let end = Point { x: 320., y: 96. };
            let mut kinds = vec![RasterOperationKind::Fill { color: colors[0], alpha_locked: false }];
            for (shape, reverse) in layer_core::GradientShape::ALL.into_iter().flat_map(|shape| [false, true].map(move |reverse| (shape, reverse))) {
                for transparent in [false, true] {
                    let mut colors = colors;
                    if transparent { colors[1][3] = 0.; }
                    kinds.push(RasterOperationKind::Gradient { start, end, gradient: layer_core::GradientDefinition { stops: colors.into_iter().enumerate().map(|(i, rgba)| layer_core::GradientStop { position: i as f32, color: layer_core::color::RgbColor::from_linear(space, rgba).unwrap() }).collect(), interpolation: layer_core::ColorMixSpace::LinearRgb }, shape, reverse, opacity: 1., alpha_locked: false });
                }
            }
            kinds.push(RasterOperationKind::Figure(Figure {
                shape: FigureShape::Rectangle, paint: FigurePaint::Fill,
                start, end, width: 4., colors, alpha_locked: false, erase: false,
            }));
            kinds.push(RasterOperationKind::Figure(Figure {
                shape: FigureShape::Rectangle, paint: FigurePaint::Both,
                start, end, width: 4., colors, alpha_locked: false, erase: false,
            }));
            for kind in kinds {
                live.append_raster_operation(id, RasterOperation {
                    placement: Affine::IDENTITY,
                    coverage: reveal_all(live.document().composition().size, Point::default()),
                    kind: kind.clone(),
                }).unwrap();
                flush(&mut live);
                let pixels = backing(&paint(live.document()).raster);
                assert_ne!(pixels, empty, "{color:?} {kind:?}");
                // Independent straight-linear reference, including both sides
                // of the 256px page boundary and outside the figure.
                for (x, y) in [(16, 64), (128, 64), (300, 64), (368, 64)] {
                    let expected = match &kind {
                        RasterOperationKind::Fill { color, .. } => *color,
                        RasterOperationKind::Gradient { gradient, shape, reverse, .. } => {
                            let a = gradient.stops[0].color.linear_in(space).unwrap();
                            let b = gradient.stops[1].color.linear_in(space).unwrap();
                            let p = [x as f64 + 0.5 - 64., y as f64 + 0.5 - 32.];
                            let projected = (p[0]*256. + p[1]*64.) / (256.*256. + 64.*64.);
                            let t = match shape {
                                layer_core::GradientShape::Radial => p[0].hypot(p[1]) / 256f64.hypot(64.),
                                layer_core::GradientShape::Reflected => projected.abs(),
                                layer_core::GradientShape::Linear => projected,
                            }.clamp(0.,1.);
                            let t = if *reverse {1.-t} else {t};
                            let alpha = (1.-t)*f64::from(a[3]) + t*f64::from(b[3]);
                            std::array::from_fn(|i| if i == 3 { alpha as f32 } else if alpha == 0. { 0. }
                                else { (((1.-t)*f64::from(a[i])*f64::from(a[3]) + t*f64::from(b[i])*f64::from(b[3])) / alpha) as f32 })
                        },
                        RasterOperationKind::Figure(f) => if (64..320).contains(&x) {
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
                assert_eq!(backing(&paint(live.document()).raster), empty);
                assert!(live.redo().unwrap()); flush(&mut live);
                assert_eq!(backing(&paint(live.document()).raster), pixels);
                let capture = live.capture_artwork(0).unwrap();
                let mut bytes = Vec::new(); write_capture(&capture,&mut bytes).unwrap();
                let loaded = read_document(bytes);
                assert_eq!(loaded.composition().color, color);
                assert_eq!(backing(&paint(&loaded).raster), pixels);
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
            let mut document = layer_core::Document::new(PortableId::random(), 256, 256, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
            set_color(&mut document,color);
            let id = target(&document);
            let (mut input, mut live) = engine(document);
            stroke(&mut live, &mut input, 1, 60.);
            let first = paint(live.document()).raster.clone();
            let native_first = backing(&first);
            assert!(!native_first.is_empty(), "{color:?}");
            let working_first = working(live.backend(), id);
            stroke(&mut live, &mut input, 10, 68.);
            let second = paint(live.document()).raster.clone();
            let native_second = backing(&second);
            let working_second = working(live.backend(), id);
            assert_ne!(native_first, native_second, "{color:?}");
            assert!(live.undo().unwrap());
            flush(&mut live);
            assert_eq!(backing(&paint(live.document()).raster), native_first);
            assert_eq!(working(live.backend(), id), working_first, "undo {color:?}");
            assert!(live.redo().unwrap());
            flush(&mut live);
            assert_eq!(
                working(live.backend(), id),
                working_second,
                "redo {color:?}"
            );
            let capture = live.capture_artwork(0).unwrap();
            let mut bytes = Vec::new();
            write_capture(&capture,&mut bytes).unwrap();
            let loaded = read_document(bytes);
            assert_eq!(loaded.composition().color, color);
            let (mut reopened_input, mut reopened) = engine(loaded);
            assert_eq!(
                backing(&paint(reopened.document()).raster),
                native_second
            );
            assert_eq!(
                working(reopened.backend(), target(reopened.document())),
                working_second,
                "reopen {color:?}"
            );
            let next = live.document().next_stroke_id();
            for _ in reopened.document().next_stroke_id().0..next.0 { reopened.allocate_stroke_id(); }
            assert_eq!(reopened.document().next_stroke_id(), next);
            stroke(&mut live, &mut input, 20, 73.);
            stroke(&mut reopened, &mut reopened_input, 20, 73.);
            let third = backing(&paint(live.document()).raster);
            assert_eq!(
                third,
                backing(&paint(reopened.document()).raster),
                "continue {color:?}"
            );
            assert_eq!(working(live.backend(), id), working(reopened.backend(), target(reopened.document())));
            let checkpoint = live.checkpoint();
            let before_recovery = working(live.backend(), id);
            let replacement = WgpuRasterizer::new_native_headless(color).unwrap();
            let retired = live.replace_backend(replacement).unwrap();
            retired.device.destroy();
            drop(retired);
            flush(&mut live);
            assert_eq!(live.checkpoint(), checkpoint);
            assert_eq!(backing(&paint(live.document()).raster), third);
            assert_eq!(
                working(live.backend(), id),
                before_recovery,
                "replacement {color:?}"
            );
            assert!(live.undo().unwrap());
            flush(&mut live);
            assert_eq!(backing(&paint(live.document()).raster), native_second);
        }
    }
}

#[test]
fn native_gpen_keeps_original_photo_pixels_in_touched_tiles() {
    use layer_core::color::{ColorProfile, source::*};
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        for in_place in [false, true] {
            let mut document = layer_core::Document::new(PortableId::random(), 4353, 769, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
            set_color(&mut document,DocumentColor {space:RgbSpace::Srgb,depth});
            let paper = document.scene().order()[1];
            document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
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
            paint_mut(&mut document).original = Some(Arc::new(source.finish().unwrap()));
            let (mut input, mut live) = engine(document);
            let r = live.backend_mut();
            let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
            r.native_edit = Some(NativeEdit::with_mode(r, transfer, in_place));
            let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
            brush.color_rgba_linear = [1., 0., 0.7, 1. / 3.];
            live.set_brush(brush).unwrap();
            stroke(&mut live, &mut input, 1, 60.);
            let stored = backing(&paint(live.document()).raster);
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

fn packet<'a>(layers: &'a Document, reset: bool) -> FramePacket<'a> {
    FramePacket { view: view(), reset_layers: reset, ..crate::test_support::packet(layers.scene(), layers.composition().size) }
}
fn restored_fixture(r: &mut WgpuRasterizer) -> Document {
    let color = r.document_color();
    let mut layers = paint_document([256 * 17, 256], "many tiles");
    set_color(&mut layers,color);
    let mut data = RasterData { watercolor: Some(layer_core::raster::RasterWatercolor { wet_edge: 0.5, burnt_edge: 0.5, edge_width: 2. }), ..Default::default() };
    for plane in [
        RasterPlane::Color,
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
            .filter(|(key, _)| key.plane == RasterPlane::WatercolorWetness)
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
    let mut mask = reveal_all(layers.composition().size, Default::default());
    mask.source.raster = RasterRevision::backed(mask_data);
    set_mask(&mut layers,mask);
    paint_mut(&mut layers).raster = RasterRevision::backed(data);
    r.submit(packet(&layers, true)).unwrap();
    layers
}
fn mark_changed(r: &mut WgpuRasterizer, layers: &mut Document) {
    paint_mut(layers).raster = RasterRevision::pending();
    let mask = mask_target(layers);
    *layers.target_raster_mut(mask).unwrap() = RasterRevision::pending();
    for id in [target(layers), mask_target(layers)] {
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
    let before = backing(&paint(&layers).raster);
    let mask_before = backing(mask_raster(&layers));
    let scratch = r.native_edit.as_ref().unwrap().storage_bytes();
    for _ in 0..3 {
        mark_changed(&mut r, &mut layers);
        while !r.raster_ready() {
            std::thread::yield_now();
        }
        r.submit(packet(&layers, false)).unwrap();
        assert_eq!(backing(&paint(&layers).raster), before);
        assert_eq!(
            backing(mask_raster(&layers)),
            mask_before
        );
        assert_eq!(r.native_edit.as_ref().unwrap().storage_bytes(), scratch);
    }
    // An unchanged pending revision reuses every backing ticket, with no copies.
    let previous = paint(&layers).raster.wait_data().unwrap();
    paint_mut(&mut layers).raster = RasterRevision::pending();
    while !r.raster_ready() {
        std::thread::yield_now();
    }
    r.submit(packet(&layers, false)).unwrap();
    let current = paint(&layers).raster.wait_data().unwrap();
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
            r.layer_masks.pages[&(mask_target(&layers), [16, 0])].texture.clone()
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
        let before = working(&r, target(&layers));
        let mask_before = working(&r, mask_target(&layers));
        mark_changed(&mut r, &mut layers);
        while !r.raster_ready() {
            std::thread::yield_now();
        }
        r.submit(packet(&layers, false)).unwrap();
        for root in [&paint(&layers).raster, mask_raster(&layers)] {
            for tile in root.wait_data().unwrap().tiles.values() {
                assert!(tile.wait_backing().is_err(), "all capture chunks must fail");
            }
            assert!(!root.host_backed());
        }
        assert_eq!(working(&r, target(&layers)), before);
        assert_eq!(working(&r, mask_target(&layers)), mask_before);
        // The last host-backed checkpoint is independent of the failed frame.
        assert!(paint(&checkpoint).raster.host_backed());
        assert!(mask_raster(&checkpoint).host_backed());
        let mut replacement = WgpuRasterizer::new_native_headless(color).unwrap();
        replacement.submit(packet(&checkpoint, true)).unwrap();
        assert_eq!(
            backing(&paint(&checkpoint).raster),
            backing(&replacement.raster.as_ref().unwrap().targets[&target(&checkpoint)].revision)
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
    let before = working(&r, target(&layers));
    mark_changed(&mut r, &mut layers);
    while !r.raster_ready() {
        std::thread::yield_now();
    }
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    let frame = r
        .encode_native_rasters(layers.scene(), &mut encoder)
        .unwrap()
        .unwrap();
    let tickets: Vec<_> = frame
        .publications
        .iter()
        .flat_map(|p| p.data.tiles.values().cloned())
        .collect();
    drop(frame);
    drop(encoder);
    assert!(paint(&layers).raster.wait_data().is_err());
    assert!(mask_raster(&layers).wait_data().is_err());
    assert!(
        tickets
            .iter()
            .all(|tile| matches!(tile.try_backing(), Some(Err(_))))
    );
    assert_eq!(working(&r, target(&layers)), before);
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
    let mut layers = paint_document([256; 2], "sRGB code/coverage grid");
    paint_mut(&mut layers).raster = RasterRevision::backed(RasterData {
        tiles: [(
            key,
            RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()),
        )]
        .into(),
        watercolor: None,
    });
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let submit = |r: &mut WgpuRasterizer, layers: &Document, reset| {
        r.submit(FramePacket {
            document_extent: [256, 256],
            ..packet(layers, reset)
        })
        .unwrap();
    };
    submit(&mut r, &layers, true);
    let samples = working(&r, target(&layers));
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
        paint_mut(&mut layers).raster = RasterRevision::pending();
        r.raster
            .as_mut()
            .unwrap()
            .targets
            .get_mut(&target(&layers))
            .unwrap()
            .changed
            .insert([0, 0]);
        while !r.raster_ready() {
            std::thread::yield_now();
        }
        submit(&mut r, &layers, false);
        assert_eq!(backing(&paint(&layers).raster)[&key], bytes);
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
    let before = backing(&paint(&layers).raster);
    let mask_before = backing(mask_raster(&layers));
    while !r.raster_ready() { std::thread::yield_now(); }
    let presentation = r.prioritize_raster_presentation();
    mark_changed(&mut r, &mut layers);
    r.submit(packet(&layers, false)).unwrap();
    let first = layers.clone();
    let page = r.paint_layers[0].pages.iter().find(|p| p.coordinate == [0, 0]).unwrap().active().texture.clone();
    set_pixel(&r, &page, &[0.; 4]);
    let mask = r.layer_masks.pages[&(mask_target(&layers), [0, 0])].texture.clone();
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
        for root in [&paint(&snapshot).raster, mask_raster(&snapshot)] {
            assert!(root.wait_data().unwrap().tiles.values().all(|t| t.try_backing().is_none()));
        }
    }
    assert_eq!(r.raster_buffers.transfer.load(Ordering::Relaxed), 0);
    drop(presentation);
    assert_eq!(backing(&paint(&first).raster), before);
    assert_eq!(backing(mask_raster(&first)), mask_before);
    let mut expected = before;
    expected.get_mut(&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }).unwrap()[..8].fill(0);
    let mut mask_expected = mask_before;
    mask_expected.get_mut(&TileKey { plane: RasterPlane::Mask, coordinate: [0, 0] }).unwrap()[..2].copy_from_slice(&32768u16.to_le_bytes());
    assert_eq!(backing(&paint(&second).raster), expected);
    assert_eq!(backing(mask_raster(&second)), mask_expected);
    for root in [&paint(&layers).raster, mask_raster(&layers)] {
        assert!(root.wait_data().unwrap().tiles.values().all(|t| t.wait_backing().is_err()));
    }
}

#[test]
fn pending_native_save_and_immediate_undo_finish_after_presentation_releases_backing() {
    let mut document = layer_core::Document::new(PortableId::random(), 256, 256, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    set_color(&mut document,DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 });
    let (mut input, mut live) = engine(document);
    while !live.backend().raster_ready() { std::thread::yield_now(); }
    let presentation = live.backend().prioritize_raster_presentation();
    stroke(&mut live, &mut input, 1, 60.);
    let first = paint(live.document()).raster.clone();
    stroke(&mut live, &mut input, 10, 75.);
    let second = paint(live.document()).raster.clone();
    assert!(!first.host_backed());
    assert!(!second.host_backed());
    let capture = live.capture_artwork(0).unwrap();
    let (started, ready) = mpsc::channel();
    let save = std::thread::spawn(move || {
        started.send(()).unwrap();
        let mut bytes = Vec::new();
        write_capture(&capture,&mut bytes).unwrap();
        read_document(bytes)
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
    assert_eq!(backing(&paint(live.document()).raster), first_bytes);
    let saved = save.join().unwrap();
    assert_eq!(backing(&paint(&saved).raster), second_bytes);
    assert!(live.redo().unwrap());
    flush(&mut live);
    assert_eq!(backing(&paint(live.document()).raster), second_bytes);
}

#[test]
fn device_loss_before_deferred_native_backing_keeps_the_last_checkpoint() {
    let color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let mut layers = restored_fixture(&mut r);
    let checkpoint = layers.clone();
    let expected = backing(&paint(&checkpoint).raster);
    while !r.raster_ready() { std::thread::yield_now(); }
    let presentation = r.prioritize_raster_presentation();
    mark_changed(&mut r, &mut layers);
    r.submit(packet(&layers, false)).unwrap();
    r.device.poll(wgpu::PollType::Wait { submission_index: r.last_submission.clone(), timeout: Some(READBACK_TIMEOUT) }).unwrap();
    assert!(!paint(&layers).raster.host_backed());
    r.device.destroy();
    drop(presentation);
    for root in [&paint(&layers).raster, mask_raster(&layers)] {
        assert!(root.wait_data().unwrap().tiles.values().all(|tile| tile.wait_backing().is_err()));
    }
    assert!(paint(&checkpoint).raster.host_backed());
    let mut replacement = WgpuRasterizer::new_native_headless(color).unwrap();
    replacement.submit(packet(&checkpoint, true)).unwrap();
    assert_eq!(backing(&paint(&checkpoint).raster), expected);
}

#[test]
fn native_in_place_runtime_matches_candidate_fallback_for_mixed_planes() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let color = DocumentColor { space: RgbSpace::ProPhoto, depth };
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        assert!(r.native_edit.as_ref().unwrap().promoter.is_none(), "in-place feature must be active for this comparison");
        let mut layers = restored_fixture(&mut r);
        let expected = backing(&paint(&layers).raster);
        let mask_expected = backing(mask_raster(&layers));
        for in_place in [false, true] {
            let transfer = r.prepare_native_transfer(color.space).unwrap();
            r.native_edit = Some(NativeEdit::with_mode(&r, transfer, in_place));
            mark_changed(&mut r, &mut layers);
            while !r.raster_ready() { std::thread::yield_now(); }
            r.submit(packet(&layers, false)).unwrap();
            assert_eq!(backing(&paint(&layers).raster), expected);
            assert_eq!(backing(mask_raster(&layers)), mask_expected);
        }
    }
}

#[test]
fn watercolor_prediction_canonicalizes_coverage_with_candidate_fallback() {
    for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F32] {
        let color = DocumentColor { depth, ..Default::default() };
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        let transfer = r.prepare_native_transfer(color.space).unwrap();
        r.native_edit = Some(NativeEdit::with_mode(&r, transfer, false));
        assert!(r.native_edit.as_ref().unwrap().promoter.is_some());
        crate::layer_tests::placement::watercolor_prediction_and_commit_with_renderer(r, 256, false);
    }
}

#[test]
fn native_gradient_dither_changes_integer_code_boundaries_without_bias() {
    use layer_core::{Affine,GradientDefinition,GradientShape,GradientStop,ColorMixSpace,RasterOperation,RasterOperationKind,Point,SceneScope};
    use layer_core::color::RgbColor;
    use crate::tests::native_effects::{empty_document,insert_effect,insert_source};
    for route in ["tool","fill","map"] {for space in RgbSpace::ALL {for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F32] {
        let maximum=if depth==SampleDepth::U16 {65535u16}else{255u16};
        let first=if depth==SampleDepth::U16 {25000.}else{100.};
        let mut document=layer_core::Document::new(PortableId::random(),1024,128,layer_core::DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
        set_color(&mut document,DocumentColor {space,depth});let id=target(&document);
        let (_,mut live)=engine(document);
        let gradient=GradientDefinition {stops:[first,first+8.].into_iter().enumerate().map(|(i,code)|GradientStop {position:i as f32,color:RgbColor::new(space,[(code/f64::from(maximum)) as f32,(code/f64::from(maximum)) as f32,(code/f64::from(maximum)) as f32,1.]).unwrap()}).collect(),interpolation:ColorMixSpace::Classic};
        let kind=if route=="tool" {RasterOperationKind::Gradient {gradient:gradient.clone(),shape:GradientShape::Linear,reverse:false,opacity:1.,start:Point::default(),end:Point {x:1024.,y:0.},alpha_locked:false}} else {
            let definition=layer_core::bundled_effect_catalog().get(if route=="fill" {"gradient_fill"}else{"gradient_map"}).unwrap();
            let mut instance=layer_core::EffectInstance::new(definition.program());instance.set("gradient",layer_core::EffectValue::Gradient(gradient)).unwrap();
            if route=="fill" {instance.set("angle",layer_core::EffectValue::Number(0.)).unwrap();}
            let mut members=empty_document([1024,128],DocumentColor {space,depth});
            insert_effect(&mut members,instance);
            if route=="map" {
                let interpretation=layer_core::color::source::SourceInterpretation {channels:layer_core::color::source::SourceChannels::Rgba,depth:SampleDepth::U8,profile:layer_core::color::ColorProfile::Builtin(space),profile_assumed:false};
                let mut builder=layer_core::color::source::SourceBuilder::new([1024,128],interpretation,16*1024*1024).unwrap();
                let row:Vec<u8>=(0..1024).flat_map(|x|{let code=(x*255/1023) as u8;[code,code,code,255]}).collect();
                for _ in 0..128 {builder.push_row(&row).unwrap();}
                insert_source(&mut members,"Gray input",Arc::new(builder.finish().unwrap()));
            }
            RasterOperationKind::Bake {scene:members.snapshot(),scope:SceneScope::Members(members.scene().order().to_vec().into()),offset:Point::default()}
        };
        let mut exported=None;
        let mut float_pixels=None;
        let mut tiles=None;
        if route=="map" {
            let RasterOperationKind::Bake {scene,scope,..}=&kind else {unreachable!()};
            let mut capture=live.backend().snapshot_gpu().capture_scene(scene.clone(),scope.clone(),Default::default()).unwrap();
            if depth==SampleDepth::F32 {float_pixels=Some(capture.read_region([0,0,1024,128]).unwrap());}
            else {
                let interpretation=layer_core::color::source::SourceInterpretation {channels:layer_core::color::source::SourceChannels::Rgba,depth,profile:layer_core::color::ColorProfile::Builtin(space),profile_assumed:false};
                let mut png=Vec::new();capture.write_png(&mut png,&interpretation,Default::default(),None).unwrap();
                let source=layer_color::photo::read_photo(std::io::Cursor::new(png),Default::default()).unwrap();
                let mut reader=source.rows();let mut row=vec![0;1024*depth.bytes()*4];let mut bytes=Vec::new();
                for y in 0..128 {reader.read(y,&mut row).unwrap();bytes.extend_from_slice(&row);}
                exported=Some(bytes);
            }
        } else {
            live.append_raster_operation(id,RasterOperation {placement:Affine::IDENTITY,coverage:reveal_all([1024,128],Point::default()),kind}).unwrap();flush(&mut live);
            tiles=Some(backing(&paint(live.document()).raster));
        }
        let mut actual=Vec::new();let mut plain=Vec::new();
        for y in 0..128u32 {for x in 0..1024u32 {
            let key=TileKey {plane:layer_core::raster::RasterPlane::Color,coordinate:[x/256,0]};
            let (bytes,offset)=if let Some(bytes)=&exported {(bytes.as_slice(),((y*1024+x) as usize)*depth.bytes()*4)}
                else if let Some(tiles)=&tiles {(tiles[&key].as_slice(),((y*256+x%256) as usize)*depth.bytes()*4)}
                else {(&[][..],0)};
            let t=if route=="map" {f64::from(x*255/1023)/255.} else {(f64::from(x)+0.5)/1024.};
            if depth==SampleDepth::F32 {
                let linear=if let Some(pixels)=&float_pixels {pixels[(y*1024+x) as usize][0]}else{f32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap())};
                let encoded=space.encode(f64::from(linear))*f64::from(maximum);
                assert!((encoded-(first+8.*t)).abs()<0.0005,"{route} {space:?} {x},{y}: {encoded}");
                continue;
            }
            let code=if depth==SampleDepth::U8 {u16::from(bytes[offset])}else{u16::from_le_bytes(bytes[offset..offset+2].try_into().unwrap())};actual.push(code);
            plain.push((first+8.*t).round() as u16);
        }}
        if depth==SampleDepth::F32 {continue; }
        let changed=plain.iter().zip(&actual).filter(|(a,b)|a!=b).count();
        let mean=plain.iter().zip(&actual).map(|(a,b)|f64::from(*b)-f64::from(*a)).sum::<f64>()/plain.len() as f64;
        let varied=(0..1024).filter(|&x|(1..128).any(|y|actual[y*1024+x]!=actual[x])).count();
        let runs=|image:&Vec<u16>| {let mut count=0usize;for row in image.chunks_exact(1024){count+=1+row.windows(2).filter(|pair|pair[0]!=pair[1]).count();}image.len() as f64/count as f64};
        let plain_band=runs(&plain);let dither_band=runs(&actual);
        let range=actual.iter().fold([u16::MAX,0],|[a,b],&v|[a.min(v),b.max(v)]);
        println!("DITHER_CODES {route} {space:?} {depth:?} changed={changed}/{} row_varied_columns={varied}/1024 mean_delta={mean} band_px={plain_band}->{dither_band} range={range:?}",plain.len());
        assert!(dither_band<plain_band/8.);
        assert!(changed>plain.len()/8);assert!(varied>512);assert!(mean.abs()<0.02);
        assert!(plain.iter().zip(&actual).all(|(a,b)|a.abs_diff(*b)<=1));
    }}}
}
