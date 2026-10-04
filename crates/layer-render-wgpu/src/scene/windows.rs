//! Bounded native physical-filter execution. Every window uses the complete
//! declared support of its layer graph. Queue completion retires its images and
//! staging before the next allocation; there is no reduced-precision edit cache.
use super::*;

// Minimum filter reserve in the shared composition allowance. Larger admitted
// documents retain their inputs; windows are the fallback when they do not fit.
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
        for side in [2048u32, 1024, 512, PAGE_SIZE] {
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

    /// Output regions covering `dirty`, each with the window of input its
    /// filters read.
    pub(super) fn regions(self, dirty: PixelRect) -> impl Iterator<Item = (PixelRect, PixelRect)> {
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
    fn gaussian_window_admission_covers_stacked_support_and_real_pixel_cost() {
        use layer_core::EffectValue;
        let extent=[9504,6336];
        for sigma in [21f32,85.] {for count in [1u32,3,6] {for limit in [256u64<<20,512<<20] {
            let layers:Vec<_>=(0..count).map(|i| {
                let mut layer=Layer::paint(LayerId(u64::from(i)+1),"Gaussian");layer.kind=LayerKind::Effect;
                let mut effect=EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
                effect.set("sigma",EffectValue::Number(sigma)).unwrap();layer.effect=Some(std::sync::Arc::new(effect));layer
            }).collect();
            let radius=2*(sigma*3.).ceil() as u32*count;
            let bytes_per_pixel=u64::from(2*count+1)*16;
            let smallest=extent.map(|v|v.min(PAGE_SIZE+2*radius));
            let minimum=u64::from(smallest[0])*u64::from(smallest[1])*bytes_per_pixel;
            let plan=Plan::new(&layers,extent,limit);
            if minimum>limit {assert!(plan.is_err(),"sigma={sigma} depth={count} minimum={minimum} cap={limit}");continue;}
            let plan=plan.unwrap().unwrap();let mut covered=0;let mut halo_pixels=0;let mut windows=0;
            for (output,input) in plan.regions(PixelRect::full(extent)) {
                assert_eq!(input,output.expand(radius,extent));
                let bytes=input.area()*bytes_per_pixel;
                assert!(bytes<=limit,"sigma={sigma} depth={count} actual input={input:?} bytes={bytes} cap={limit}");
                assert_eq!(bytes,Scene::capture_image_bound(&layers,input));
                covered+=output.area();halo_pixels+=input.area();windows+=1;
            }
            assert_eq!(covered,u64::from(extent[0])*u64::from(extent[1]));
            if sigma==21.&&count==1&&limit==256<<20 {
                assert_eq!(windows,20,"the admitted61MP composition must use20 large windows");
                let old=Plan{side:1024,radius,extent};
                let old_inputs:u64=old.regions(PixelRect::full(extent)).map(|(_,input)|input.area()).sum();
                assert_eq!(old.regions(PixelRect::full(extent)).count(),70);
                assert!(halo_pixels<old_inputs,"larger admitted windows reduce duplicated support pixels");
                println!("61MP sigma21 windows={windows} input pixels={halo_pixels} prior1024 input pixels={old_inputs}");
            }
        }}}
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
