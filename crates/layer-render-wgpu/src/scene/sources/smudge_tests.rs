use super::*;
use layer_core::{Point, Rect};

#[test]
fn stored_source_smudge_stroke_promotes_neighbors_once_for_the_slot_lifetime() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let source = stored_code_source(depth, 5);
        let (mut r, doc, target) = stored_code_renderer(&source);
        let extent = source.extent;
        crate::test_support::submit_published(&mut r, crate::test_support::packet(doc.scene(), extent));
        crate::test_support::complete(&r);
        crate::test_support::evict_sources(&mut r);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let originals: Vec<_> = (0..5).map(|x| stored_code_tile(&mut r, &source, x, &mut encoder)).collect();
        r.uploads.finish(&encoder); encoder.submit(&r.queue); crate::test_support::complete(&r);
        let identities: Vec<_> = originals.iter().enumerate().map(|(x, tile)| {
            assert_eq!(tile.texture.format(), stored_code_format(depth), "{depth:?}, page {x}");
            let sources = r.source_tiles.borrow();
            let slot = sources.slots.iter().position(|slot| slot.view == tile.view).unwrap();
            (slot, sources.lease(&tile.view).unwrap())
        }).collect();
        let sample_misses = r.device.source_samples.stats().misses;
        let mut style = crate::tests::test_style(BrushExecution::Smudge);
        style.blender = layer_core::BrushBlender { reach: 0., rate: 1., ..Default::default() };
        let pass_plan = BrushPassPlan::for_device(&style, &r.device);
        let mut promoted_views = Vec::new();
        for step in 0..4 {
            let center = [384. + step as f32 * 8., 128.];
            let mut dab = crate::tests::test_dab(center, [1., 0., 0., 1.], 1.);
            dab.motion = [8., 0.];
            dab.material = [0., 0.5, 1., 0.];
            let damage = Rect { min: Point { x: center[0] - 24., y: center[1] - 24. },
                max: Point { x: center[0] + 24., y: center[1] + 24. } };
            let mut batch = crate::test_support::dab_batch(target, style.clone(), damage);
            batch.stroke_start = step == 0;
            batch.stroke_end = step == 3;
            assert_eq!(BrushPassPlan::for_device(&batch.style, &r.device), pass_plan);
            crate::test_support::submit_published(&mut r, FramePacket {
                dabs: &[dab], dab_batches: &[batch], composite_all: false,
                ..crate::test_support::packet(doc.scene(), extent)
            });
            crate::test_support::complete(&r);
            {
                let sources = r.source_tiles.borrow();
                for (x, (index, lease)) in identities.iter().enumerate() {
                    let slot = &sources.slots[*index];
                    assert!(slot.key.as_ref().unwrap().matches(&Key::Image(Arc::downgrade(&source), [x as u32, 0])));
                    assert!(Arc::ptr_eq(lease, &sources.lease(&slot.view).unwrap()));
                    let format = if x <= 2 { wgpu::TextureFormat::Rgba32Float } else { stored_code_format(depth) };
                    assert_eq!(slot.layer.format(), format, "{depth:?}, dab {step}, page {x}");
                    if x <= 2 {
                        if step == 0 {
                            assert_ne!(slot.view, originals[x].view);
                            promoted_views.push(slot.view.clone());
                        } else {
                            assert_eq!(slot.view, promoted_views[x], "a warm dab must reuse the promoted source slot");
                        }
                    } else {
                        assert_eq!(slot.view, originals[x].view, "untouched source pages stay encoded");
                    }
                }
            }
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            r.read_raw_neighborhood(target, [1, 0], [Some([-1, 0]), Some([0, 0]), Some([1, 0])], &mut encoder).unwrap();
            assert_eq!(encoder.pass_count(), 0, "the real smudge dab already prepared its immutable neighbors");
            r.uploads.finish(&encoder); encoder.submit(&r.queue);
        }
        assert_eq!(r.device.source_samples.stats().misses, sample_misses);
        let layer = r.paint_layers.iter().find(|layer| layer.id == target).unwrap();
        assert!(layer.pages.iter().all(|page| page.coordinate[1] == 0 && page.coordinate[0] <= 2));
        let painted = layer.pages.iter().find(|page| page.coordinate == [1, 0]).unwrap();
        let expected=r.current_raster_version(target,layer_core::raster::RasterPlane::Color,painted.coordinate).unwrap();
        let read=r.read_layer_color(target,painted.coordinate,&expected,None).unwrap();
        assert!(crate::test_support::float_pixels(&r, &read.value().texture).iter().any(|pixel| pixel[3] > 0.));
    }
}
