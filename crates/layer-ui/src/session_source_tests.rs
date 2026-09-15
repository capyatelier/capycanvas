#[test]
fn retained_import_transform_clear_and_undo_keep_source_precision() {
    use layer_core::color::{ColorProfile, IntegerDepth, RgbSpace, source::*};
    let mut builder = SourceBuilder::new(
        [3, 2],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
            profile_assumed: false,
        },
        1024,
    )
    .unwrap();
    let row: Vec<u8> = [
        65535u16, 0, 1023, 1, 123, 45678, 32101, 0, 3, 65534, 65535, 32767,
    ]
    .into_iter()
    .flat_map(u16::to_le_bytes)
    .collect();
    builder.push_row(&row).unwrap();
    builder.push_row(&row).unwrap();
    let source = builder.finish().unwrap();
    let mut session = session();
    let before = session.engine.document().clone();
    assert!(
        session
            .import_layer_source("Photo", source.clone())
            .unwrap_err()
            .contains("does not support")
    );
    assert_eq!(session.engine.document(), &before);
    session.engine.backend_mut().tiled_sources = true;
    assert!(
        session
            .import_layer_source("bad\nname", source.clone())
            .is_err()
    );
    assert_eq!(session.engine.document(), &before);
    session
        .import_layer_source("Photo", source.clone())
        .unwrap();
    let id = session.engine.document().active_layer;
    let check = |session: &UiSession<Recorder>| {
        assert_eq!(session.engine.document().color, Default::default());
        let layer = session.engine.document().layer(id).unwrap();
        assert_eq!(layer.source.as_deref(), Some(&source));
        assert!(layer.raster.is_empty());
        assert!(layer.asset.is_none());
    };
    check(&session);
    assert!(session.command(CommandId::ScaleRotate).enabled);
    let revision = session.engine.document().revision;
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::ScaleRotate,
        })
        .unwrap();
    assert!(session.operation.active());
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::CancelTransform,
        })
        .unwrap();
    assert_eq!(session.engine.document().revision, revision);
    check(&session);
    session
        .dispatch(UiAction::Layer {
            action: LayerAction::Clear { id: id.0 },
        })
        .unwrap();
    assert!(
        session
            .engine
            .document()
            .layer(id)
            .unwrap()
            .source
            .is_none()
    );
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
    check(&session);
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
    assert!(
        session
            .engine
            .document()
            .layer(id)
            .unwrap()
            .source
            .is_none()
    );
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
    assert!(session.engine.document().layer(id).is_none());
    assert_eq!(session.engine.document().active_layer, before.active_layer);
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
    check(&session);
    let project = session.capture_project_recovery().unwrap();
    let mut archive = Vec::new();
    project.write(&mut archive).unwrap();
    let restored =
        layer_core::Project::read(std::io::Cursor::new(archive), Default::default()).unwrap();
    assert_eq!(
        restored.document.layer(id).unwrap().source.as_deref(),
        Some(&source)
    );
}

#[test]
fn source_profile_repair_preserves_samples_and_baked_edits() {
    use layer_core::{
        color::{ColorProfile, IntegerDepth, RgbSpace, source::*},
        raster::*,
    };
    use std::sync::Arc;
    let mut builder = SourceBuilder::new(
        [1, 1],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: true,
        },
        1024,
    )
    .unwrap();
    let samples: Vec<u8> = [65535u16, 12345, 54321, 1]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    builder.push_row(&samples).unwrap();
    let mut session = session();
    session.engine.backend_mut().tiled_sources = true;
    session
        .import_layer_source("Original", builder.finish().unwrap())
        .unwrap();
    let id = session.engine.document().active_layer;
    session.state.platform = Platform::Gtk;
    let change = session.dispatch(UiAction::Layer {
        action: LayerAction::RepairSourceProfile { id: id.0 },
    }).unwrap();
    assert_ne!(change.regions & regions::HOST, 0, "native dialog must be serviced");
    let request_id = session.state.requests.last().unwrap().id;
    session.complete_document_request(request_id, Ok(false)).unwrap();

    let original = session
        .engine
        .document()
        .layer(id)
        .unwrap()
        .source
        .clone()
        .unwrap();
    let mut corrected = (*original).clone();
    corrected.interpretation.profile = ColorProfile::Builtin(RgbSpace::DisplayP3);
    corrected.interpretation.profile_assumed = false;
    let before = session.engine.document().clone();
    let mut invalid = corrected.clone();
    invalid.extent[0] += 1;
    assert!(session.repair_layer_source(id, &original, invalid).is_err());
    assert_eq!(session.engine.document(), &before);
    let preview = session.preview_layer_source(id, &original, corrected.clone()).unwrap();
    assert_eq!(session.engine.document(), &before, "preview does not mutate live content or IDs");
    assert_eq!(
        session
            .repair_layer_source(id, &original, corrected.clone())
            .unwrap(),
        id
    );
    assert_eq!(&preview.document, session.engine.document(), "preview and Apply use the same complete edit");
    let after = session
        .engine
        .document()
        .layer(id)
        .unwrap()
        .source
        .as_ref()
        .unwrap();
    assert_eq!(after.interpretation, corrected.interpretation);
    assert!(Arc::ptr_eq(
        after.tiles.values().next().unwrap(),
        original.tiles.values().next().unwrap()
    ));
    assert!(
        session
            .repair_layer_source(id, &original, corrected.clone())
            .unwrap_err()
            .contains("source changed")
    );
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
    assert!(Arc::ptr_eq(
        session
            .engine
            .document()
            .layer(id)
            .unwrap()
            .source
            .as_ref()
            .unwrap(),
        &original
    ));

    // Simulate an immutable committed paint tile plus live layer/mask metadata.
    let mut layer = session.engine.document().layer(id).unwrap().clone();
    layer.properties.offset = layer_core::Point { x: 4., y: 9. };
    layer.mask = Some(layer_core::LayerMask::reveal_all(
        session.engine.allocate_layer_id(),
        Default::default(),
    ));
    let descriptor = session.engine.document().color.paint_descriptor();
    let bytes = vec![55; descriptor.byte_len([TILE_SIZE; 2]).unwrap()];
    let key = TileKey {
        plane: RasterPlane::Color,
        coordinate: [0, 0],
    };
    let tile = RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap());
    layer.raster = RasterRevision::backed(RasterData {
        tiles: [(key, tile)].into(),
        watercolor: None,
    });
    session
        .engine
        .apply_edit(layer_core::Edit::ReplaceLayer(Box::new(layer.clone())))
        .unwrap();
    let original_project = session.capture_project_recovery().unwrap();
    let preview = session.preview_layer_source(id, &original, corrected.clone()).unwrap();
    assert_eq!(session.engine.document(), &original_project.document);
    let next_id = session
        .repair_layer_source(id, &original, corrected.clone())
        .unwrap();
    assert_ne!(next_id, id);
    assert_eq!(&preview.document, session.engine.document());
    let doc = session.engine.document();
    assert_eq!(
        doc.layer(id).unwrap(),
        &layer,
        "baked layer and its mask stay intact"
    );
    let next = doc.layer(next_id).unwrap();
    assert_eq!(next.properties.offset, layer.properties.offset);
    assert_eq!(next.source.as_deref(), Some(&corrected));
    assert!(next.raster.is_empty());
    assert!(next.mask.is_none());
    assert_eq!(doc.active_layer, next_id);
    let mut archive = Vec::new();
    session
        .capture_project_recovery()
        .unwrap()
        .write(&mut archive)
        .unwrap();
    let reopened =
        layer_core::Project::read(std::io::Cursor::new(archive), Default::default()).unwrap();
    assert_eq!(
        reopened.document.layer(id).unwrap().source.as_deref(),
        Some(original.as_ref())
    );
    assert_eq!(
        reopened.document.layer(next_id).unwrap().source.as_deref(),
        Some(&corrected)
    );
    assert_eq!(
        reopened
            .document
            .layer(id)
            .unwrap()
            .raster
            .wait_data()
            .unwrap()
            .tiles[&key]
            .wait_backing()
            .unwrap()
            .decode()
            .unwrap(),
        bytes
    );
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
    assert!(session.engine.document().layer(next_id).is_none());
    assert_eq!(session.engine.document().layer(id).unwrap(), &layer);
    assert_eq!(session.engine.document().active_layer, id);
    session
        .dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
    assert_eq!(session.engine.document().layer(id).unwrap(), &layer);
    assert_eq!(
        session
            .engine
            .document()
            .layer(next_id)
            .unwrap()
            .source
            .as_deref(),
        Some(&corrected)
    );
}
