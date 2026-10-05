use super::*;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};

#[test]
fn retained_paint_base_without_live_pages_matches_identity_and_translated_queries() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F32 };
    let pixel = |x, y| [u8::from(x % 3 == 0) as f32, u8::from(y % 5 == 0) as f32, 1., 1.];
    let source = crate::test_support::depth_source([64, 32], SampleDepth::F32, color.space, 1 << 20,
        pixel);
    let mut document = crate::tests::native_effects::empty_document([64, 32], color);
    crate::tests::native_effects::insert_source(&mut document, "Retained paint base", source);
    let snapshot = document.scene().snapshot(Default::default());
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    assert!(r.paint_layers.is_empty());
    let extent = [32, 16];
    for offset in [[0, 0], [-7, -5]] {
        let (texture, _) = create_color_target(&r.device, extent, "retained paint query");
        let mut scene = Scene::new(&r);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        scene.capture_region(&mut r, crate::test_support::packet(snapshot.view().with_offset64(offset.map(f64::from)), extent),
            &texture, PixelRect::full(extent), Output::Artwork(None), &mut encoder).unwrap();
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        let expected: Vec<_> = (0..extent[1]).flat_map(|y| (0..extent[0]).map(move |x|
            pixel((x as i32 - offset[0]) as u32, (y as i32 - offset[1]) as u32))).collect();
        assert!(crate::test_support::max_error(&crate::test_support::float_pixels(&r, &texture), &expected) < 1e-6, "offset={offset:?}");
        assert!(r.paint_layers.is_empty());
    }
}

#[test]
fn coarse_graph_retains_negative_effect_inputs_and_matches_larger_reference() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F32 };
    let source = crate::test_support::depth_source([64, 32], SampleDepth::F32, color.space, 1 << 20,
        |x, y| if (20..32).contains(&x) && (8..24).contains(&y) { [0.7, 0.2, 0.1, 1.] } else { [0.; 4] });
    for watercolor in [false, true] {
        let make = |extent, translation| {
            let mut doc = crate::tests::native_effects::empty_document(extent, color);
            let mut blur = layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
            Arc::make_mut(&mut blur.program).resolution = layer_core::EffectResolution::Display;
            blur.set("sigma", layer_core::EffectValue::Number(4.)).unwrap();
            crate::tests::native_effects::insert_effect(&mut doc, blur);
            let paint = crate::tests::native_effects::insert_source(&mut doc, "retained source", source.clone());
            doc.artwork.occurrences.get_mut(paint).unwrap().translation = translation;
            if watercolor {
                use layer_core::raster::*;
                let layer_core::SourceTarget::Paint(handle) = doc.scene().source_target(paint).unwrap() else { panic!("paint target"); };
                let descriptor = RasterPlane::WatercolorWetness.descriptor(color);
            let tile = RasterTile::backed(TileBlob::encode(descriptor, &vec![255; descriptor.byte_len([256; 2]).unwrap()]).unwrap());
                doc.artwork.paint.get_mut(handle).unwrap().raster = RasterRevision::backed(RasterData {
                    watercolor: Some(RasterWatercolor { wet_edge: 0.9, burnt_edge: 0.6, edge_width: 8. }),
                    tiles: BTreeMap::from([(TileKey { plane: RasterPlane::WatercolorWetness, coordinate: [0; 2] }, tile)]),
                });
            }
            crate::tests::native_effects::refresh(&mut doc);
            doc
        };
        let document = make([32, 32], layer_core::Point { x: -32., y: 0. });
        let reference = make([96, 64], layer_core::Point { x: 0., y: 16. });
        let evaluate = |doc: &layer_core::Document| {
            let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
            let mut packet = crate::test_support::packet(doc.scene(), doc.composition().size);
            packet.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
            r.submit(packet).unwrap();
            let cache = r.scale_display.as_ref().unwrap();
            assert!(cache.evaluation == Evaluation::Display);
            assert_eq!(cache.plan.level, 2);
            (cache.plan.size, tests::display_pixels(&r))
        };
        let (size, actual) = evaluate(&document);
        let (reference_size, reference) = evaluate(&reference);
        let expected: Vec<_> = (0..size[1]).flat_map(|y| (0..size[0]).map(move |x| ((y + 4) * reference_size[0] + x + 8) as usize))
            .map(|index| reference[index]).collect();
        assert!(actual.iter().any(|pixel| pixel[3] > 0.01));
        assert!(crate::test_support::max_error(&actual, &expected) < 3e-5, "watercolor={watercolor}");
    }
}
