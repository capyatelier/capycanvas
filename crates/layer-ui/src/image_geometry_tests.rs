/// A drawing without paper whose paint layer holds, in each listed tile, full
/// alpha inside the tile-local rectangle. Each tile's pixels differ, so every
/// tile decodes separately.
fn content_session(size: [u32; 2], tiles: &[([u32; 2], [u32; 4])]) -> UiSession<Recorder> {
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey, TILE_SIZE};
    let mut doc = Document::new("content", size[0], size[1]);
    doc.layers.retain(|l| l.kind != layer_core::LayerKind::Background);
    let descriptor = RasterPlane::Color.descriptor(doc.color);
    doc.layers[0].raster = RasterRevision::backed(RasterData {
        tiles: tiles
            .iter()
            .enumerate()
            .map(|(i, (coordinate, [x0, y0, x1, y1]))| {
                let mut bytes = vec![0; descriptor.byte_len([TILE_SIZE; 2]).unwrap()];
                for y in *y0..*y1 {
                    for x in *x0..*x1 {
                        let pixel = (y * TILE_SIZE + x) as usize * 4;
                        bytes[pixel..pixel + 4].copy_from_slice(&[i as u8, 40, 60, 255]);
                    }
                }
                let tile = RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap());
                (TileKey { plane: RasterPlane::Color, coordinate: *coordinate }, tile)
            })
            .collect(),
        watercolor: None,
    });
    let mut s = UiSession::new(Recorder::default(), doc, [1600, 1000], Platform::Gtk).unwrap();
    s.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
    invoke(&mut s, CommandId::FitCanvas);
    s.frame(1, 1).unwrap();
    s
}

fn size_of(s: &UiSession<Recorder>) -> [u32; 2] {
    [s.engine.document().width, s.engine.document().height]
}

/// Frames until a bounds scan running on a worker has finished.
fn settle_bounds(s: &mut UiSession<Recorder>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut frame = 100;
    while s.content_bounds.busy() {
        assert!(s.wants_continuous_frames(), "a scan in flight keeps frames coming");
        assert!(std::time::Instant::now() < deadline, "the bounds scan finishes");
        std::thread::sleep(std::time::Duration::from_millis(2));
        frame += 1;
        s.frame(frame, frame).unwrap();
    }
}

#[test]
fn rotating_a_non_square_image_right_turns_every_pixel_in_one_step() {
    let mut s = crop_session();
    let paint = s.engine.document().layers[0].id;
    rectangle_selection(&mut s, [100., 100., 300., 200.]);
    let before = s.engine.document().clone();
    let center = on_surface(&s, Point { x: 500., y: 400. });
    for command in [CommandId::RotateImageLeft, CommandId::RotateImageRight, CommandId::RotateImage180, CommandId::FlipImageHorizontal, CommandId::FlipImageVertical] {
        assert!(s.command(command).enabled, "{command:?}");
    }
    invoke(&mut s, CommandId::RotateImageRight);
    s.frame(20, 20).unwrap();
    assert_eq!(size_of(&s), [800, 1000]);
    let [(id, transform)] = resampled(&mut s).try_into().unwrap();
    assert_eq!(id, paint);
    assert_eq!(transform.interpolation, layer_core::Interpolation::Nearest, "an exact permutation");
    assert_eq!(s.engine.document().target_extent(paint), [1000, 1000], "a square scratch extent");
    let turn = layer_core::Affine([0., 1., -1., 0., 800., 0.]);
    assert_eq!(s.engine.document().selection, Some(before.selection.as_ref().unwrap().transformed(turn).unwrap()));
    near_point(on_surface(&s, turn.map(Point { x: 500., y: 400. })), center, 0.01);
    invoke(&mut s, CommandId::Undo);
    s.frame(21, 21).unwrap();
    assert_eq!(size_of(&s), [1000, 800]);
    assert_eq!(s.engine.document().layers, before.layers, "one undo step");
    assert_eq!(s.engine.document().selection, before.selection);
}

#[test]
fn flipping_twice_and_turning_four_times_come_back_to_the_start() {
    let mut s = crop_session();
    let paint = s.engine.document().layers[0].id;
    let before = s.engine.document().clone();
    let corner = before.layer_transform(paint).map(Point { x: 10.5, y: 20.5 });
    for (command, times) in [(CommandId::FlipImageHorizontal, 2), (CommandId::FlipImageVertical, 2), (CommandId::RotateImage180, 2), (CommandId::RotateImageLeft, 4)] {
        let mut p = Point { x: 10.5, y: 20.5 };
        for step in 0..times {
            invoke(&mut s, command);
            s.frame(30 + step, 30 + step).unwrap();
            let [(_, transform)] = resampled(&mut s).try_into().unwrap();
            p = transform.as_affine().unwrap().map(p);
        }
        assert_eq!(size_of(&s), [1000, 800], "{command:?}");
        near_point(s.engine.document().layer_transform(paint).map(p), corner, 1e-3);
    }
}

#[test]
fn trim_shrinks_to_the_visible_pixels_and_reveal_all_brings_hidden_pixels_back() {
    let mut s = content_session([1000, 800], &[([0, 0], [0, 0, 1, 1]), ([1, 1], [10, 20, 100, 200]), ([2, 1], [0, 0, 30, 5])]);
    assert!(s.command(CommandId::Trim).enabled);
    invoke(&mut s, CommandId::Trim);
    s.frame(10, 10).unwrap();
    assert_eq!(size_of(&s), [542, 456], "from the pixel at 0,0 to the right and bottom edges");
    invoke(&mut s, CommandId::Undo);
    let paint = s.engine.document().layers[0].id;
    s.layer_edit(layer_core::Edit::ReplaceLayer(Box::new(layer_core::Layer {
        properties: layer_core::LayerProperties { offset: Point { x: -100., y: 0. }, ..s.engine.document().layers[0].properties.clone() },
        ..s.engine.document().layers[0].clone()
    }))).unwrap();
    s.frame(11, 11).unwrap();
    invoke(&mut s, CommandId::Trim);
    s.frame(12, 12).unwrap();
    assert_eq!(size_of(&s), [276, 200], "only the pixels on the canvas count; the one at 0,0 lies beyond its left edge");
    assert_eq!(s.engine.document().layer(paint).unwrap().properties.offset, Point { x: -266., y: -256. });
    invoke(&mut s, CommandId::RevealAll);
    s.frame(13, 13).unwrap();
    let doc = s.engine.document();
    assert_eq!([doc.width, doc.height], [542, 456], "every pixel, including the one beyond the edge");
    let all = layer_core::ContentBoundsRequest::new(doc, layer_core::ContentScope::All);
    let found = all.scan(&Default::default(), layer_core::ScanBudget::Worker).unwrap().unwrap();
    assert_eq!([found.min.x, found.min.y, found.max.x, found.max.y], [0., 0., 542., 456.], "the pixels reach every edge");
    invoke(&mut s, CommandId::RevealAll);
    assert_eq!(notice_text(&s), Some("Every pixel is already on the canvas"));
    invoke(&mut s, CommandId::Undo);
    invoke(&mut s, CommandId::Undo);
    s.frame(14, 14).unwrap();
    assert_eq!(size_of(&s), [1000, 800], "Trim and Reveal All are one step each");
}

#[test]
fn trim_and_fit_content_refuse_when_nothing_is_visible() {
    let mut s = content_session([600, 400], &[([0, 0], [0, 0, 0, 0])]);
    invoke(&mut s, CommandId::Trim);
    assert_eq!(notice_text(&s), Some("There are no visible pixels to trim to"));
    assert_eq!(s.command_disabled_reason(CommandId::CropFitContent).as_deref(), Some("Choose the Crop tool first"));
    invoke(&mut s, CommandId::Crop);
    assert!(!s.command(CommandId::Trim).enabled);
    assert_eq!(s.command_disabled_reason(CommandId::Trim).as_deref(), Some("Apply or cancel the crop first"));
    invoke(&mut s, CommandId::CropFitContent);
    assert_eq!(notice_text(&s), Some("There are no visible pixels to fit the crop to"));
    let mut full = content_session([256, 256], &[([0, 0], [0, 0, 256, 256])]);
    invoke(&mut full, CommandId::Trim);
    assert_eq!(notice_text(&full), Some("The visible pixels already reach every edge of the canvas"));
}

#[test]
fn fit_content_sets_the_crop_to_the_content_including_pixels_beyond_the_canvas() {
    let mut s = content_session([600, 400], &[([0, 0], [50, 60, 256, 256]), ([1, 1], [0, 0, 20, 10]), ([2, 1], [0, 0, 250, 1])]);
    invoke(&mut s, CommandId::Crop);
    invoke(&mut s, CommandId::CropRatioSquare);
    let bar: Vec<_> = s.state.tool_actions.iter().map(|a| a.command).collect();
    assert!(bar.contains(&CommandId::CropFitContent), "Fit Content is on the crop bar");
    invoke(&mut s, CommandId::CropFitContent);
    let frame = crop_frame(&s);
    assert_eq!((frame.center, frame.size, frame.angle), (Point { x: 406., y: 163. }, [712., 206.], 0.));
    assert!(s.command(CommandId::CropRatioFree).selected, "the frame is exact, so the ratio is free");
    invoke(&mut s, CommandId::ApplyTransform);
    s.frame(10, 10).unwrap();
    assert_eq!(size_of(&s), [712, 206], "the canvas grows to hold the pixels past its edge");
}

#[test]
fn a_large_scan_finishes_on_a_worker_and_a_changed_drawing_cancels_it() {
    let tiles: Vec<_> = (0..6u32).flat_map(|y| (0..6u32).map(move |x| ([x, y], [1, 1, 250, 250]))).collect();
    let mut s = content_session([1536, 1536], &tiles);
    invoke(&mut s, CommandId::Trim);
    assert!(s.content_bounds.busy(), "the edge tiles are more than the UI thread decodes");
    assert_eq!(size_of(&s), [1536, 1536], "nothing changes until the scan finishes");
    settle_bounds(&mut s);
    assert_eq!(size_of(&s), [1529, 1529]);
    assert!(s.state.notice.is_none());

    let mut s = content_session([1536, 1536], &tiles);
    invoke(&mut s, CommandId::RevealAll);
    assert!(s.content_bounds.busy());
    let selection = layer_core::Selection::polygon(layer_core::Rect { min: Point { x: 1., y: 1. }, max: Point { x: 9., y: 9. } }.corners().to_vec()).unwrap();
    s.layer_edit(layer_core::Edit::SetSelection(Some(selection))).unwrap();
    settle_bounds(&mut s);
    assert_eq!(size_of(&s), [1536, 1536]);
    assert_eq!(notice_text(&s), Some("Reveal All stopped because the drawing changed"));
}
