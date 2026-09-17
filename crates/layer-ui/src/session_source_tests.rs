#[test]
fn photo_batch_placement_is_atomic_ordered_and_transforms_retained_sources_together() {
    use layer_core::{Affine, color::{IntegerDepth, source::*}};
    let source = |extent: [u32; 2]| {
        let mut builder = SourceBuilder::new(extent, SourceInterpretation {
            channels: SourceChannels::Rgba, depth: IntegerDepth::U8,
            profile: Default::default(), profile_assumed: false,
        }, 1 << 20).unwrap();
        for _ in 0..extent[1] { builder.push_row(&vec![255; extent[0] as usize * 4]).unwrap(); }
        builder.finish().unwrap()
    };
    let first = source([600, 400]);
    let second = source([100, 300]);
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new("batch", 200, 150), [800, 600]).unwrap();
    session.layer_interaction.selected = std::collections::BTreeSet::from([LayerId(1), LayerId(2)]);
    let selected = session.layer_interaction.selected.clone();
    let original = session.engine.document().clone();
    assert!(session.place_layer_sources(vec![("First".into(), first.clone()), ("bad\nname".into(), second.clone())],
        None, None).is_err());
    assert_eq!(session.engine.document(), &original, "failed batch reserves no live IDs and inserts nothing");
    let images = || vec![("First".into(), first.clone()), ("Second".into(), second.clone())];
    session.place_layer_sources(images(), Some(Point { x: 75., y: 55. }), None).unwrap();
    let doc = session.engine.document();
    let ids: Vec<_> = doc.layers[..2].iter().map(|layer| layer.id).collect();
    assert_eq!(doc.layers[..2].iter().map(|l| l.name.as_ref()).collect::<Vec<_>>(), ["First", "Second"]);
    assert_eq!(session.layer_interaction.selected, ids.iter().copied().collect());
    assert!(!session.engine.can_undo());
    let before = doc.layers[..2].to_vec();
    session.set_transform_control("transform_x", 20.).unwrap();
    session.set_transform_control("transform_width", 2.).unwrap();
    for (i, layer) in session.engine.document().layers[..2].iter().enumerate() {
        assert_eq!(layer.source, before[i].source);
        assert_eq!(layer.properties.placement.map(Point {
            x: layer.source.as_ref().unwrap().extent[0] as f32 / 2.,
            y: layer.source.as_ref().unwrap().extent[1] as f32 / 2.,
        }), Point { x: 95., y: 55. });
        assert!((layer.properties.placement.0[0] - before[i].properties.placement.0[0] * 2.).abs() < 0.00001);
        assert!(layer.raster.is_empty());
    }
    invoke(&mut session, CommandId::PlacementOriginalSize);
    for layer in &session.engine.document().layers[..2] {
        assert_eq!(&layer.properties.placement.0[..4], &Affine::IDENTITY.0[..4]);
    }
    invoke(&mut session, CommandId::CancelTransform);
    assert_eq!(session.engine.document().layers, original.layers);
    assert_eq!(session.layer_interaction.selected, selected);
    assert!(!session.engine.can_undo());

    session.place_layer_sources(images(), None, None).unwrap();
    let committed = session.engine.document().layers.clone();
    invoke(&mut session, CommandId::ApplyTransform);
    invoke(&mut session, CommandId::Undo);
    assert_eq!(session.engine.document().layers, original.layers);
    assert!(!session.engine.can_undo(), "the whole batch is one artwork history entry");
    invoke(&mut session, CommandId::Redo);
    assert_eq!(session.engine.document().layers, committed);
    let mut bytes = Vec::new();
    session.capture_project_recovery().unwrap().write(&mut bytes).unwrap();
    let restored = layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap();
    assert_eq!(restored.document.layers, committed);
}

#[test]
fn photo_drop_destination_respects_groups_locks_clipping_and_parent_offsets() {
    use crate::{ImageLayerDestination, LayerDropPosition};
    use layer_core::{Layer, LayerKind, color::{IntegerDepth, source::*}};
    let mut doc = Document::new("drop", 200, 150);
    let group_id = doc.allocate_layer_id();
    let mut group = Layer::paint(group_id, "Group");
    group.kind = LayerKind::Group;
    group.properties.offset = Point { x: 40., y: -10. };
    doc.layers[0].properties.parent = Some(group_id);
    doc.layers.insert(0, group);
    let clipped_id = doc.allocate_layer_id();
    let mut clipped = Layer::paint(clipped_id, "Clipped");
    clipped.properties.parent = Some(group_id);
    clipped.properties.clipped = true;
    doc.layers.insert(1, clipped);
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [800, 600]).unwrap();
    let mut builder = SourceBuilder::new([2, 1], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: IntegerDepth::U8,
        profile: Default::default(), profile_assumed: false,
    }, 1024).unwrap();
    builder.push_row(&[255; 8]).unwrap();
    let source = builder.finish().unwrap();
    assert_eq!(session.image_layer_drop_hint(group_id.0, 0.5), Some(LayerDropPosition::Into));
    assert_eq!(session.image_layer_drop_hint(2, 0.9), Some(LayerDropPosition::Above));
    assert_eq!(session.image_layer_drop_hint(1, 0.1), None, "insertion must not change a clipping base");
    assert_eq!(session.image_layer_drop_hint(clipped_id.0, 0.9), None);
    assert_eq!(session.image_layer_drop_hint(clipped_id.0, 0.1), Some(LayerDropPosition::Above));
    let original = session.engine.document().clone();
    session.place_layer_sources(vec![("Menu import".into(), source.clone())], None, None).unwrap();
    assert_eq!(session.engine.document().layers[1].name.as_ref(), "Menu import",
        "default Import goes above the complete clipped stack");
    assert!(session.engine.document().layers[2].properties.clipped);
    invoke(&mut session, CommandId::CancelTransform);
    assert_eq!(session.engine.document().layers, original.layers);
    for position in [LayerDropPosition::Into, LayerDropPosition::Below] {
        session.place_layer_sources(vec![("Photo".into(), source.clone())], None,
            Some(ImageLayerDestination { target: group_id, position })).unwrap();
        let doc = session.engine.document();
        let layer = doc.layer(doc.active_layer).unwrap();
        assert_eq!(layer.properties.parent, (position == LayerDropPosition::Into).then_some(group_id));
        assert_eq!(doc.layer_transform(layer.id).map(Point { x: 1., y: 0.5 }), Point { x: 100., y: 75. });
        assert_eq!(doc.layers.iter().position(|l| l.id == layer.id).unwrap(),
            if position == LayerDropPosition::Into { 1 } else { 3 });
        invoke(&mut session, CommandId::CancelTransform);
        assert_eq!(session.engine.document().layers, original.layers);
    }
    let mut group = session.engine.document().layer(group_id).unwrap().clone();
    group.properties.locked = true;
    session.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(group))).unwrap();
    assert_eq!(session.image_layer_drop_hint(group_id.0, 0.5), None);
    assert_eq!(session.image_layer_drop_hint(1, 0.1), None);
    assert_eq!(session.image_layer_drop_hint(group_id.0, 0.1), Some(LayerDropPosition::Above));
    assert!(session.place_layer_sources(vec![("Photo".into(), source)], None,
        Some(ImageLayerDestination { target: group_id, position: LayerDropPosition::Into })).is_err());
}

#[test]
fn rejected_photo_placement_start_keeps_the_previous_tool_and_selection() {
    use layer_core::color::{IntegerDepth, source::*};
    let mut builder = SourceBuilder::new([2, 1], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: IntegerDepth::U8,
        profile: Default::default(), profile_assumed: false,
    }, 1024).unwrap();
    builder.push_row(&[255; 8]).unwrap();
    let mut doc = Document::new("locked photo", 200, 150);
    doc.layers[0].source = Some(std::sync::Arc::new(builder.finish().unwrap()));
    doc.layers[0].properties.locked = true;
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, doc, [800, 600]).unwrap();
    let before = session.engine.document().clone();
    let selected = session.layer_interaction.selected.clone();
    let tool = session.layer_interaction.tool;
    assert!(session.begin_layer_placement(None).unwrap_err().contains("locked"));
    assert!(!session.operation.active());
    assert_eq!(session.layer_interaction.tool, tool);
    assert_eq!(session.layer_interaction.selected, selected);
    assert_eq!(session.engine.document(), &before);
    assert!(session.capture_project_recovery().is_ok(), "failed start must not block Save/recovery");
}

#[test]
fn photo_placement_fit_cancel_apply_original_size_and_one_step_history() {
    use layer_core::{Affine, color::{IntegerDepth, source::*}};
    let mut builder = SourceBuilder::new([600, 400], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: IntegerDepth::U16,
        profile: Default::default(), profile_assumed: false,
    }, 8 * 1024 * 1024).unwrap();
    for _ in 0..400 { builder.push_row(&[255; 600 * 8]).unwrap(); }
    let source = builder.finish().unwrap();
    let mut session = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },
        Document::new("placement", 200, 150), [800, 600]).unwrap();
    let original = session.engine.document().clone();
    session.place_layer_source("Photo", source.clone(), None).unwrap();
    let placed = session.engine.document().layer(session.engine.document().active_layer).unwrap();
    let id = placed.id;
    let matrix = placed.properties.placement;
    assert!((matrix.0[0] - 1. / 3.).abs() < 0.00001);
    assert_eq!(matrix.map(Point { x: 300., y: 200. }), Point { x: 100., y: 75. });
    assert!(session.operation.placing());
    assert!(!session.engine.can_undo(), "provisional import has no artwork history");
    assert!(session.capture_project_recovery().is_err(), "pending placement cannot enter recovery/save");
    assert!(session.state.tool_actions.iter().any(|a| a.command == CommandId::PlacementOriginalSize));
    invoke(&mut session, CommandId::CancelTransform);
    assert_eq!(session.engine.document().layers, original.layers);
    assert_eq!(session.engine.document().active_layer, original.active_layer);
    assert!(!session.engine.can_undo());

    session.place_layer_source("Photo", source.clone(), Some(Point { x: 60., y: 80. })).unwrap();
    let id2 = session.engine.document().active_layer;
    assert_ne!(id, id2, "cancelled IDs are never reused");
    let placed = session.engine.document().layer(id2).unwrap().clone();
    invoke(&mut session, CommandId::ApplyTransform);
    assert!(!session.operation.active());
    assert!(session.engine.document().layer(id2).unwrap().raster.is_empty());
    invoke(&mut session, CommandId::Undo);
    assert_eq!(session.engine.document().layers, original.layers);
    assert!(!session.engine.can_undo(), "insertion and placement are one history entry");
    invoke(&mut session, CommandId::Redo);
    assert_eq!(session.engine.document().layer(id2).unwrap().properties, placed.properties);

    let project = session.capture_project_recovery().unwrap();
    let mut bytes = Vec::new();
    project.write(&mut bytes).unwrap();
    let restored = layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap();
    let mut reopened = UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, restored.document, [800, 600]).unwrap();
    let thumbnail_revision = reopened.state.layers.iter().find(|l| l.id == id2.0).unwrap().paint_revision;
    invoke(&mut reopened, CommandId::ScaleRotate);
    invoke(&mut reopened, CommandId::PlacementOriginalSize);
    invoke(&mut reopened, CommandId::ApplyTransform);
    let layer = reopened.engine.document().layer(id2).unwrap();
    assert_eq!(&layer.properties.placement.0[..4], &Affine::IDENTITY.0[..4]);
    assert_eq!(layer.source.as_deref(), Some(&source));
    assert!(layer.raster.is_empty());
    assert!(layer.pending_operations.is_empty());
    assert!(reopened.engine.transform_preview().is_none(), "whole photo placement bypasses raster transforms");
    assert_eq!(layer.properties.placement.map(Point { x: 300., y: 200. }), Point { x: 60., y: 80. });
    assert_ne!(reopened.state.layers.iter().find(|l| l.id == id2.0).unwrap().paint_revision,
        thumbnail_revision, "accepted geometry invalidates the host's cached preview");
    invoke(&mut reopened, CommandId::Undo);
    assert_eq!(reopened.state.layers.iter().find(|l| l.id == id2.0).unwrap().paint_revision,
        thumbnail_revision, "undo publishes the restored preview identity");
}

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
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios] {
        source_profile_repair_preserves_samples_and_baked_edits_on(platform);
    }
}
fn source_profile_repair_preserves_samples_and_baked_edits_on(platform: Platform) {
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
    session.state.platform = platform;
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
    let original_preview_revision = session.state.layers.iter().find(|l| l.id == id.0).unwrap().paint_revision;
    assert_eq!(
        session
            .repair_layer_source(id, &original, corrected.clone())
            .unwrap(),
        id
    );
    assert_eq!(&preview.document, session.engine.document(), "preview and Apply use the same complete edit");
    let corrected_preview_revision = session.state.layers.iter().find(|l| l.id == id.0).unwrap().paint_revision;
    assert_ne!(corrected_preview_revision, original_preview_revision,
        "repair must refresh the Layers image even when source samples and raster history are unchanged");
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
    assert_ne!(session.state.layers.iter().find(|l| l.id == id.0).unwrap().paint_revision,
        corrected_preview_revision, "Undo must refresh the restored source interpretation");

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

#[test]
fn rasterizing_an_image_preserves_full_extent_edits_masks_and_history() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios] {
        rasterizing_an_image_preserves_full_extent_edits_masks_and_history_on(platform);
    }
}
fn rasterizing_an_image_preserves_full_extent_edits_masks_and_history_on(platform: Platform) {
    use layer_core::{color::source::*, raster::*};
    use std::sync::Arc;
    let mut session = session();
    session.state.platform = platform;
    session.engine.backend_mut().tiled_sources = true;
    // The retained image is larger than the document. Materializing it must not
    // crop off-canvas pixels or bake/shift the layer's existing paint and mask.
    let mut builder = SourceBuilder::new([1500, 2], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: Default::default(), profile: Default::default(), profile_assumed: true,
    }, 1024 * 1024).unwrap();
    for _ in 0..2 { builder.push_row(&vec![127; 1500 * 4]).unwrap(); }
    session.import_layer_source("Reference", builder.finish().unwrap()).unwrap();
    let id = session.engine.document().active_layer;
    let original = session.engine.document().layer(id).unwrap().source.clone().unwrap();
    let mut layer = session.engine.document().layer(id).unwrap().clone();
    layer.properties.offset = layer_core::Point { x: -550., y: 3.5 };
    layer.mask = Some(layer_core::LayerMask::reveal_all(session.engine.allocate_layer_id(), layer_core::Point { x: 17., y: 3. }));
    let key = TileKey { plane: RasterPlane::Color, coordinate: [0, 0] };
    layer.raster = RasterRevision::backed(RasterData { tiles: [(key, RasterTile::backed(TileBlob::encode(session.engine.document().color.paint_descriptor(), &vec![51; 256 * 256 * 4]).unwrap()))].into(), watercolor: None });
    session.engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(layer.clone()))).unwrap();
    let before = session.engine.document().clone();
    let mut converted = (*original).clone(); converted.kind = SourceKind::Rasterized; converted.interpretation.profile_assumed = false;
    let converted = Arc::new(converted);
    let mut invalid = (*converted).clone(); invalid.extent[0] = 1499;
    assert!(session.apply_rasterized_source(id, &original, Arc::new(invalid)).is_err());
    assert_eq!(session.engine.document(), &before);
    let change = session.dispatch(UiAction::Layer { action: LayerAction::RasterizeSource { id: id.0 } }).unwrap();
    assert_ne!(change.regions & regions::HOST, 0);
    let request = session.state.requests.last().unwrap().id;
    session.complete_document_request(request, Ok(false)).unwrap();
    let preview = session.preview_rasterized_source(id, &original, converted.clone()).unwrap();
    assert_eq!(session.engine.document(), &before);
    session.apply_rasterized_source(id, &original, converted.clone()).unwrap();
    assert_eq!(&preview.document, session.engine.document());
    let mut expected = layer.clone(); expected.source = Some(converted.clone());
    assert_eq!(session.engine.document().layer(id).unwrap(), &expected);
    assert!(!session.command(CommandId::RepairSourceProfile).enabled);
    assert!(!session.command(CommandId::RasterizeSource).enabled);
    let mut bytes = Vec::new(); session.capture_project_recovery().unwrap().write(&mut bytes).unwrap();
    let restored = layer_core::Project::read(std::io::Cursor::new(bytes), Default::default()).unwrap();
    assert_eq!(restored.document.layer(id).unwrap().source.as_deref(), Some(converted.as_ref()));
    assert_eq!(restored.document.layer(id).unwrap().mask, expected.mask);
    session.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    assert_eq!(session.engine.document().layer(id).unwrap(), &layer);
    assert!(session.command(CommandId::RepairSourceProfile).enabled);
    session.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
    assert_eq!(session.engine.document().layer(id).unwrap(), &expected);
}

#[test]
fn source_admission_counts_aggregate_ownership_before_mutating_document_or_ids() {
    use layer_core::{Edit, ProjectLimits};
    use layer_core::color::{ColorProfile, source::*};
    use std::sync::Arc;
    let fixture = || {
        let mut builder = SourceBuilder::new([256, 256], SourceInterpretation {
            channels: SourceChannels::Rgba, depth: Default::default(),
            profile: Default::default(), profile_assumed: false,
        }, 1024 * 1024).unwrap();
        let mut random = 17u32;
        for _ in 0..256 {
            let row: Vec<u8> = (0..1024).map(|_| {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                (random >> 24) as u8
            }).collect();
            builder.push_row(&row).unwrap();
        }
        builder.finish().unwrap()
    };
    let source = fixture();
    let limits = ProjectLimits { asset_bytes: source.resident_bytes() as u64 + 1024, ..Default::default() };
    let mut session = session();
    session.engine.backend_mut().tiled_sources = true;
    session.import_layer_source_with_limits("First", source.clone(), limits).unwrap();
    session.engine.apply_edit(Edit::SetLayerOpacity { id: session.engine.document().active_layer, opacity: 0.5 }).unwrap();
    session.engine.undo().unwrap();
    let before = session.engine.document().clone();
    let checkpoint = session.engine.checkpoint();
    assert!(session.engine.can_redo());
    let error = session.import_layer_source_with_limits("Independent allocation", fixture(), limits).unwrap_err();
    assert!(error.contains("memory limit"), "{error}");
    assert_eq!(session.engine.document(), &before);
    assert_eq!(session.engine.checkpoint(), checkpoint);
    assert!(session.engine.can_redo());
    // Identical bytes in a different allocation count twice; shared tile backing
    // counts once even when separate layers own different image-index objects.
    session.import_layer_source_with_limits("Shared backing", source, limits).unwrap();
    session.capture_project_recovery().unwrap().pruned().unwrap().validate(limits).unwrap();
    let before = session.engine.document().clone();
    let mut repaired = before.layer(before.active_layer).unwrap().clone();
    let mut source = repaired.source.as_ref().unwrap().as_ref().clone();
    source.interpretation.profile = ColorProfile::Icc(vec![19; 16384].into());
    repaired.source = Some(Arc::new(source));
    let error = session.source_edit_candidate(&Edit::ReplaceLayer(Box::new(repaired)), None, limits).unwrap_err();
    assert!(error.contains("memory limit"), "{error}");
    assert_eq!(session.engine.document(), &before);
}
