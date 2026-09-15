//! Bounded native physical-filter execution. Every window uses the complete
//! declared support of its layer graph. Queue completion retires its images and
//! staging before the next allocation; there is no reduced-precision edit cache.
use super::*;

// An implementation ceiling, not a measured device/workload qualification. The
// native GTK mode remains disabled until total residency and latency pass.
pub(crate) const DEFAULT_IMAGE_PIXEL_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Plan {
    side: u32,
    radius: u32,
    extent: [u32; 2],
}
impl Plan {
    /// None retains the ordinary incremental image cache. A window plan is
    /// needed only when its conservative full-image allocation exceeds the cap.
    pub fn new(
        layers: &[Layer],
        extent: [u32; 2],
        limit: u64,
    ) -> Result<Option<Self>, GpuRasterError> {
        if extent.contains(&0) {
            return Err(GpuRasterError::InvalidExtent);
        }
        let full = Scene::capture_image_bound(layers, PixelRect::full(extent));
        if full <= limit {
            return Ok(None);
        }
        let radius = layers
            .iter()
            .filter(|l| images::visible(layers, l))
            .filter_map(|l| l.effect.as_ref())
            .try_fold(0u32, |r, e| Some(r.saturating_add(e.damage_radius()?)))
            .ok_or_else(|| GpuRasterError::Color(format!(
                "Document-wide filters require up to {full} bytes of image pixels; the live filter limit is {limit} bytes"
            )))?;
        // Bounded recording as well as bounded textures. Whole output pages
        // preserve the existing compositor's tile ownership at window seams.
        for side in [1024u32, 512, PAGE_SIZE] {
            let window = PixelRect::full(
                extent.map(|v| v.min(side.saturating_add(radius.saturating_mul(2)))),
            );
            if Scene::capture_image_bound(layers, window) <= limit {
                return Ok(Some(Self {
                    side,
                    radius,
                    extent,
                }));
            }
        }
        Err(GpuRasterError::Color(format!(
            "Filter halos exceed the live image pixel limit of {limit} bytes even for one output tile"
        )))
    }

    fn regions(self, dirty: PixelRect) -> impl Iterator<Item = (PixelRect, PixelRect)> {
        let extent = self.extent;
        let radius = self.radius;
        let side = self.side;
        (dirty.min_y()..dirty.max_y())
            .step_by(side as usize)
            .flat_map(move |y| {
                (dirty.min_x()..dirty.max_x())
                    .step_by(side as usize)
                    .map(move |x| {
                        let output = PixelRect::new(
                            x,
                            y,
                            x.saturating_add(side).min(dirty.max_x()),
                            y.saturating_add(side).min(dirty.max_y()),
                        );
                        (output, output.expand(radius, extent))
                    })
            })
    }
}

impl Scene {
    pub(super) fn compose_windows(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        dirty: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
        overlay: bool,
        plan: Plan,
    ) -> Result<(), GpuRasterError> {
        let extent = packet.document_extent;
        let dirty = if packet.composite_all
            || packet.reset_layers
            || self
                .images
                .metadata_changed(packet.layers, packet.view.background_rgba_linear)
            || packet.layers.iter().any(|l| {
                images::visible(packet.layers, l) && l.effect.as_ref().is_some_and(|e| e.animated())
            }) {
            PixelRect::full(extent)
        } else {
            dirty.expand(plan.radius, extent)
        };
        if dirty.is_empty() {
            return Ok(());
        }
        let dirty = PixelRect::new(
            dirty.min_x() / PAGE_SIZE * PAGE_SIZE,
            dirty.min_y() / PAGE_SIZE * PAGE_SIZE,
            dirty
                .max_x()
                .div_ceil(PAGE_SIZE)
                .saturating_mul(PAGE_SIZE)
                .min(extent[0]),
            dirty
                .max_y()
                .div_ceil(PAGE_SIZE)
                .saturating_mul(PAGE_SIZE)
                .min(extent[1]),
        );
        self.effects.retain(packet.layers);
        // A preceding frame or paint upload can still refer to the old cache.
        // Retire it before replacing a formerly full-document image set.
        Self::submit_chunk(r, encoder, "before bounded filters")?;
        r.metrics.image_window_submissions += 1;
        let result = (|| {
            for (output, window) in plan.regions(dirty) {
                self.jobs.clear();
                self.images = images::ImageStages::default();
                self.record_count = 0;
                self.image_window = Some(window);
                self.update_images(r, packet, window, encoder)?;
                r.metrics.image_window_peak_bytes = r
                    .metrics
                    .image_window_peak_bytes
                    .max(self.images.storage_bytes());
                self.compose_pixels(r, packet, output, encoder, overlay, None)?;
                Self::submit_chunk(r, encoder, "after bounded filter window")?;
                r.metrics.image_window_submissions += 1;
            }
            Ok(())
        })();
        self.image_window = None;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{EffectInstance, EffectPass, EffectSampling};

    fn adjustment(sampling: EffectSampling) -> Layer {
        let mut p = (*layer_core::bundled_effect_catalog()
            .get("exposure")
            .unwrap()
            .program())
        .clone();
        p.passes = vec![EffectPass {
            entry: p.entry.clone(),
            sampling,
        }]
        .into();
        let mut l = Layer::paint(LayerId(1), "bounded filter");
        l.kind = LayerKind::Effect;
        l.effect = Some(std::sync::Arc::new(EffectInstance::new(
            std::sync::Arc::new(p),
        )));
        l
    }

    #[test]
    fn photo_windows_cover_output_and_complete_halos_within_cap() {
        let mut layers = vec![adjustment(EffectSampling::Neighborhood { radius: 19 }); 5];
        for (i, l) in layers.iter_mut().enumerate() {
            l.id = LayerId(i as u64 + 1);
        }
        for extent in [[6000, 4000], [8256, 5504], [8192, 7324], [32768, 257]] {
            let plan = Plan::new(&layers, extent, DEFAULT_IMAGE_PIXEL_BYTES)
                .unwrap()
                .unwrap();
            let mut covered = 0;
            let mut previous = None;
            for (output, window) in plan.regions(PixelRect::full(extent)) {
                covered += output.area();
                assert_eq!(window, Scene::capture_window(&layers, output, extent));
                assert!(Scene::capture_image_bound(&layers, window) <= DEFAULT_IMAGE_PIXEL_BYTES);
                assert_eq!(output.min_x() % PAGE_SIZE, 0);
                assert_eq!(output.min_y() % PAGE_SIZE, 0);
                if let Some(old) = previous {
                    assert!(output.intersect(old).is_empty());
                }
                previous = Some(output);
            }
            assert_eq!(covered, extent[0] as u64 * extent[1] as u64);
        }
    }

    #[test]
    fn oversized_global_or_halo_dependencies_are_explicit_and_hidden_ones_are_excluded() {
        let extent = [8192, 7324];
        let mut global = adjustment(EffectSampling::Document);
        assert!(
            Plan::new(&[global.clone()], extent, DEFAULT_IMAGE_PIXEL_BYTES)
                .unwrap_err()
                .to_string()
                .contains("Document-wide")
        );
        global.visible = false;
        assert!(
            Plan::new(&[global], extent, DEFAULT_IMAGE_PIXEL_BYTES)
                .unwrap()
                .is_none()
        );
        let halo = adjustment(EffectSampling::Neighborhood { radius: 4096 });
        assert!(
            Plan::new(&[halo], extent, DEFAULT_IMAGE_PIXEL_BYTES)
                .unwrap_err()
                .to_string()
                .contains("halos")
        );
        assert!(
            Plan::new(
                &[adjustment(EffectSampling::Document)],
                [256, 256],
                DEFAULT_IMAGE_PIXEL_BYTES
            )
            .unwrap()
            .is_none()
        );
    }
}
