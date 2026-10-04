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
        layers: SceneView<'_>,
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
        let radius = stack::support(layers,0)
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

    fn adjustments(sampling: EffectSampling, count: usize, extent: [u32; 2]) -> layer_core::Document {
        use layer_core::authored::*;
        let mut artwork = Artwork::new(extent).unwrap();
        let mut program = (*layer_core::bundled_effect_catalog().get("exposure").unwrap().program()).clone();
        program.passes = vec![EffectPass { entry: program.entry.clone(), sampling }].into();
        let definition = artwork.definitions.insert(PortableId::random(), Definition { program: Arc::new(program) }).unwrap();
        let stack = artwork.compositions.get(artwork.root).unwrap().result;
        for _ in 0..count {
            let values = EffectInstance::new(artwork.definitions.get(definition).unwrap().program.clone()).values;
            let effect = artwork.effects.insert(PortableId::random(), EffectApplication { definition, values, domain: extent }).unwrap();
            let handle = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), "bounded filter")).unwrap();
            artwork.stacks.get_mut(stack).unwrap().entries.push(handle);
        }
        layer_core::Document::from_artwork(artwork).unwrap()
    }

    #[test]
    fn photo_windows_cover_output_and_complete_halos_within_cap() {
        for extent in [[6000, 4000], [8256, 5504], [8192, 7324], [32768, 257]] {
            let document = adjustments(EffectSampling::Neighborhood { radius: 19 }, 5, extent);
            let layers = document.scene();
            let plan = Plan::new(layers, extent, DEFAULT_IMAGE_PIXEL_BYTES)
                .unwrap()
                .unwrap();
            let mut covered = 0;
            let mut previous = None;
            for (output, window) in plan.regions(PixelRect::full(extent)) {
                covered += output.area();
                assert_eq!(window, Scene::capture_window(layers, output, extent));
                assert!(Scene::capture_image_bound(layers, window) <= DEFAULT_IMAGE_PIXEL_BYTES);
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
            use layer_core::authored::*;
            let mut artwork=Artwork::new(extent).unwrap();
            let stack=artwork.compositions.get(artwork.root).unwrap().result;
            for _ in 0..count {
                let mut draft=EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
                draft.set("sigma",EffectValue::Number(sigma)).unwrap();
                let definition=artwork.definitions.insert(PortableId::random(),Definition {program:draft.program}).unwrap();
                let effect=artwork.effects.insert(PortableId::random(),EffectApplication {definition,values:draft.values,domain:extent}).unwrap();
                let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(effect),"Gaussian")).unwrap();
                artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
            }
            let document=layer_core::Document::from_artwork(artwork).unwrap();
            let scene=document.scene();
            let radius=2*(sigma*3.).ceil() as u32*count;
            let bytes_per_pixel=u64::from(2*count+1)*16;
            let smallest=extent.map(|v|v.min(PAGE_SIZE+2*radius));
            let minimum=u64::from(smallest[0])*u64::from(smallest[1])*bytes_per_pixel;
            let plan=Plan::new(scene,extent,limit);
            if minimum>limit {assert!(plan.is_err(),"sigma={sigma} depth={count} minimum={minimum} cap={limit}");continue;}
            let plan=plan.unwrap().unwrap();let mut covered=0;let mut halo_pixels=0;let mut windows=0;
            for (output,input) in plan.regions(PixelRect::full(extent)) {
                assert_eq!(input,output.expand(radius,extent));
                let bytes=input.area()*bytes_per_pixel;
                assert!(bytes<=limit,"sigma={sigma} depth={count} actual input={input:?} bytes={bytes} cap={limit}");
                assert_eq!(bytes,Scene::capture_image_bound(scene,input));
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
        let mut global = adjustments(EffectSampling::Document, 1, extent);
        assert!(
            Plan::new(global.scene(), extent, DEFAULT_IMAGE_PIXEL_BYTES)
                .unwrap_err()
                .to_string()
                .contains("Document-wide")
        );
        let handle = global.scene().order()[0];
        global.artwork.occurrences.get_mut(handle).unwrap().visible = false;
        assert!(
            Plan::new(global.scene(), extent, DEFAULT_IMAGE_PIXEL_BYTES)
                .unwrap()
                .is_none()
        );
        let halo = adjustments(EffectSampling::Neighborhood { radius: 4096 }, 1, extent);
        assert!(
            Plan::new(halo.scene(), extent, DEFAULT_IMAGE_PIXEL_BYTES)
                .unwrap_err()
                .to_string()
                .contains("halos")
        );
        assert!(
            Plan::new(
                adjustments(EffectSampling::Document, 1, [256, 256]).scene(),
                [256, 256],
                DEFAULT_IMAGE_PIXEL_BYTES
            )
            .unwrap()
            .is_none()
        );
    }
}
