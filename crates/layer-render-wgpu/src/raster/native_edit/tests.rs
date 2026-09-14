use super::*;
use layer_core::color::{IntegerDepth, RgbSpace};
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
        ..Default::default()
    };
    engine.set_brush(brush).unwrap();
    flush(&mut engine);
    (input, engine)
}

#[test]
fn native_engine_paint_undo_save_reopen_and_device_replacement_share_canonical_samples() {
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
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
        depth: IntegerDepth::U16,
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
            depth: IntegerDepth::U16,
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
        depth: IntegerDepth::U16,
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
