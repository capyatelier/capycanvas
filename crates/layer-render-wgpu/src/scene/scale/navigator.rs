use super::*;

#[derive(Default)]
pub(crate) struct Navigator {
    cache: Option<Cache>,
    pending: bool,
    refreshed: f32,
    pub revision: u64,
}

impl Navigator {
    pub fn pending(&self) -> bool {
        self.pending || self.cache.as_ref().is_some_and(|c| c.submission_valid.as_ref()
            .is_some_and(|v| !v.load(std::sync::atomic::Ordering::Acquire)))
    }
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.cache.as_ref().filter(|c| c.submission_valid.as_ref()
            .is_none_or(|v| v.load(std::sync::atomic::Ordering::Acquire)))
            .and_then(|c| c.pixels.root()).map(|i| &i.view)
    }
    pub fn scale(&self) -> f32 { self.cache.as_ref().map_or(1., |c| (1 << c.plan.level) as f32) }
    pub fn storage_bytes(&self) -> u64 { self.cache.as_ref().map_or(0, Cache::storage_bytes) }

    pub fn refresh(&mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, changed: bool,
        reset: bool, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        crate::performance_trace::counter(c"Capy Navigator age us", ((packet.time_seconds - self.refreshed).max(0.) * 1e6) as u64);
        crate::performance_trace::counter(c"Capy Navigator pending", u64::from(self.pending()));
        crate::performance_trace::counter(c"Capy Navigator bytes", self.storage_bytes());
        if reset { self.cache = None; }
        self.pending |= changed;
        let missing = self.view().is_none();
        if !missing && (!self.pending() || (changed && packet.time_seconds >= self.refreshed
            && packet.time_seconds - self.refreshed < 0.05)) { return Ok(()); }
        let Some(display) = r.scale_display.as_ref() else { return Ok(()); };
        let base = display_mips::Plan::new(packet.document_extent)?;
        let source = std::iter::once(display).chain(display.overview.as_deref())
            .filter(|c| c.placed.is_none() && c.plan.bounds == PixelRect::full(packet.document_extent))
            .flat_map(|c| c.pixels.root().into_iter().chain(c.pixels.next()))
            .filter(|i| i.plan.level <= base.level || i.plan.size.iter().all(|n| *n <= display_mips::MAX_SIDE))
            .min_by_key(|i| i.plan.level.abs_diff(base.level)).cloned();
        let plan = display_mips::Plan::at(base.extent, base.level.max(source.as_ref().map_or(display.plan.level, |i| i.plan.level)));
        if self.cache.as_ref().is_none_or(|c| c.plan != plan) {
            self.cache = Some(Cache::new(r, Request { plan, evaluation: Evaluation::Display }, packet.layers.len()));
        }
        let cache = self.cache.as_mut().unwrap();
        let write = crate::submission::CacheWrite::new();
        cache.submission_valid = Some(write.validity());
        let mut scene = r.scene.take().unwrap();
        let scene_write = scene.begin_write(r);
        let mut commands = scene.scale_commands.take().unwrap_or_else(|| Commands::new(r));
        commands.begin();
        let _trace = crate::performance_trace::Span::new(c"capy.navigator_refresh");
        r.telemetry.phase_begin(13, &r.device, &r.queue, encoder);
        let result = (|| {
            if let Some(source) = source {
                cache.pixels.ensure_root(r, plan, "retained Navigator");
                if source.plan == plan {
                    encoder.copy_texture_to_texture(source.texture.as_image_copy(), cache.texture().as_image_copy(),
                        wgpu::Extent3d { width: plan.size[0], height: plan.size[1], depth_or_array_layers: 1 });
                } else {
                    let mut values = [0; 20];
                    values[..8].copy_from_slice(&[0, 0, plan.size[0], plan.size[1], plan.extent[0], plan.extent[1],
                        1 << (plan.level - source.plan.level), (source.plan.level << 8) | 8]);
                    let binding = Commands::binding(r, &source.view, &r.empty_view, cache.view());
                    commands.reduce(r, encoder, values, &binding, "retain Navigator composition")?;
                }
            } else {
                cache.valid.clear();
                cache.graph.prepare(r, packet, &scene.scale_sources, plan, 0)?;
                cache.render_graph(&mut scene, r, packet, plan.bounds,
                    &mut Encoding { encoder, commands: &mut commands }, Destination::Navigator, None)?;
            }
            commands.flush(r, encoder)
        })();
        r.telemetry.phase_end(13, encoder);
        scene.scale_commands = Some(commands);
        r.scene = Some(scene);
        result?;
        write.track(encoder);
        scene_write.track(encoder);
        self.pending = false;
        self.refreshed = packet.time_seconds;
        self.revision = self.revision.wrapping_add(1);
        crate::performance_trace::counter(c"Capy Navigator refreshes", self.revision);
        crate::performance_trace::counter(c"Capy Navigator pixels", plan.size.map(u64::from).into_iter().product());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{float_pixels, packet};
    use layer_core::{Affine, Document, ImageTransform, Point};

    fn document() -> Document {
        let mut doc = Document::new("Navigator", 1025, 513);
        doc.layers[0].source = Some(layer_core::color::source::rgba8_source([1025, 513], |x, y|
            [(x / 5) as u8, (y / 3) as u8, 40, 255]));
        doc
    }
    fn pixels(r: &WgpuRasterizer) -> Vec<[f32; 4]> {
        float_pixels(r, r.navigator.cache.as_ref().unwrap().texture())
    }
    fn pose(r: &mut WgpuRasterizer, doc: &Document, x: f32) {
        r.set_transform_preview(Some(&layer_render::TransformPreview {
            transaction: 1, layer: doc.layers[0].id, moving: true, selection: None,
            transform: ImageTransform::affine(Affine::translation(Point { x, y: 0. })),
        })).unwrap();
    }

    #[test]
    fn navigator_coalesces_poses_and_flushes_without_further_input() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut frame = packet(&doc.layers, [doc.width, doc.height]);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        r.submit(frame).unwrap();
        let original = pixels(&r);
        let revision = r.navigator.revision;
        for step in 1..=4 {
            pose(&mut r, &doc, step as f32 * 20.);
            frame.time_seconds = step as f32 * 0.01;
            r.submit(frame).unwrap();
            assert_eq!(r.navigator.revision, revision);
            assert!(r.navigator.pending() && r.has_pending_work());
        }
        assert_eq!(pixels(&r), original);
        frame.time_seconds = 0.06;
        pose(&mut r, &doc, 100.);
        r.submit(frame).unwrap();
        assert_eq!(r.navigator.revision, revision + 1);
        assert_ne!(pixels(&r), original);
        assert!(!r.navigator.pending());
        frame.time_seconds = 0.07;
        pose(&mut r, &doc, 120.);
        r.submit(frame).unwrap();
        assert!(r.has_pending_work());
        frame.time_seconds = 0.071;
        r.submit(frame).unwrap();
        assert_eq!(r.navigator.revision, revision + 2);
        assert!(!r.navigator.pending());
        let settled = pixels(&r);
        for step in 1..=3 {
            frame.time_seconds += 0.1;
            frame.view.document_to_surface[4] = step as f32 * -80.;
            r.submit(frame).unwrap();
            assert_eq!(r.navigator.revision, revision + 2);
            assert_eq!(pixels(&r), settled);
        }
        for step in 1..=20 {
            frame.time_seconds += 0.06;
            pose(&mut r, &doc, 120. + step as f32);
            r.submit(frame).unwrap();
            assert_eq!(r.navigator.revision, revision + 2 + step);
        }
        r.prepare_moving_pixels(None);
        r.set_transform_preview(None).unwrap();
        r.submit(frame).unwrap();
        r.submit(frame).unwrap();
        assert_eq!(pixels(&r), original);
    }

    #[test]
    fn navigator_history_and_blending_refresh_before_native_refinement() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut frame = packet(&doc.layers, [doc.width, doc.height]);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        r.submit(frame).unwrap();
        let original = pixels(&r);
        let mut edited = doc.layers.clone();
        edited[0].opacity = 0.3;
        let mut alternate = None;
        for (step, layers) in [&edited, &doc.layers, &edited, &doc.layers].into_iter().enumerate() {
            frame.time_seconds += 0.01;
            let edit = FramePacket { layers, ..frame };
            r.submit(edit).unwrap();
            assert!(r.navigator.pending());
            r.background_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
            r.submit(edit).unwrap();
            assert!(!r.navigator.pending());
            let actual = pixels(&r);
            if step == 0 { assert_ne!(actual, original); alternate = Some(actual); }
            else { assert_eq!(actual, if step % 2 == 0 { alternate.clone().unwrap() } else { original.clone() }); }
        }
        frame.blend_space = layer_core::BlendSpace::Perceptual;
        r.submit(frame).unwrap();
        assert!(!r.navigator.pending());
        assert_ne!(pixels(&r), original);
    }

    #[test]
    fn navigator_tracks_temporary_brush_tails_and_their_removal() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut frame = packet(&doc.layers, [doc.width, doc.height]);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        r.submit(frame).unwrap();
        let original = pixels(&r);
        let mut dab = crate::tests::test_dab([100., 100.], [0.9, 0.02, 0.1, 1.], 1.);
        dab.radii = [45.; 2];
        for (time, x) in [(0.01, 100.), (0.03, 150.), (0.06, 200.)] {
            dab.center.x = x;
            let batch = DabBatch { kind: DabBatchKind::Preview, ..crate::test_support::dab_batch(
                doc.layers[0].id, crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen), dab.bounds()) };
            frame.time_seconds = time;
            r.submit(FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame }).unwrap();
        }
        assert_ne!(pixels(&r), original);
        frame.time_seconds = 0.07;
        r.submit(frame).unwrap();
        assert!(r.navigator.pending());
        r.submit(frame).unwrap();
        assert!(!r.navigator.pending());
        assert_eq!(pixels(&r), original);
    }

    #[test]
    fn navigator_replacement_and_discarded_submission_never_publish_old_pixels() {
        let doc = document();
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut frame = packet(&doc.layers, [doc.width, doc.height]);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        r.submit(frame).unwrap();
        let original = pixels(&r);
        let mut navigator = std::mem::take(&mut r.navigator);
        {
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            navigator.refresh(&mut r, FramePacket { time_seconds: 0.1, ..frame }, true, false, &mut encoder).unwrap();
        }
        assert!(navigator.pending());
        assert!(navigator.view().is_none());
        r.navigator = navigator;
        r.submit(frame).unwrap();
        assert!(!r.navigator.pending());
        assert_eq!(pixels(&r), original);
        let mut replacement_doc = doc.clone();
        replacement_doc.layers[0].source = Some(layer_core::color::source::rgba8_source([1025, 513], |_, _| [0, 0, 255, 255]));
        let replacement = FramePacket { reset_layers: true, layers: &replacement_doc.layers, ..frame };
        r.submit(replacement).unwrap();
        let changed = pixels(&r);
        assert!(changed.iter().all(|c| c[0] == 0. && c[1] == 0. && c[2] > 0.99 && c[3] == 1.));
        let mut recovered = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        recovered.submit(replacement).unwrap();
        assert_eq!(pixels(&recovered), changed);
    }
}
