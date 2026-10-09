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
}
impl Plan {
    /// None retains the ordinary incremental image cache. A window plan is
    /// needed only when its conservative full-image allocation exceeds the cap.
    pub fn new(
        layers: SceneView<'_>,
        extent: [u32; 2],
        limit: u64,
        maximum_side: u32,
    ) -> Result<Option<Self>, GpuRasterError> {
        Self::new_cached(layers,extent,limit,maximum_side,None)
    }
    pub(super) fn new_cached(layers:SceneView<'_>,extent:[u32;2],limit:u64,maximum_side:u32,objects:Option<&object_spatial::SpatialIndex>)->Result<Option<Self>,GpuRasterError> {
        if extent.contains(&0) {
            return Err(GpuRasterError::InvalidExtent);
        }
        let full_window = images::capture_window_cached(layers, PixelRect::full(extent),objects);
        let full = Scene::capture_image_bound(layers, full_window);
        if full <= limit && (full == 0 || full_window.size()?.into_iter().all(|side| side <= maximum_side)) {
            return Ok(None);
        }
        stack::support(layers,0)
            .ok_or_else(|| GpuRasterError::Color(format!(
                "Document-wide filters require up to {full} bytes of image pixels; the live filter limit is {limit} bytes"
            )))?;
        // Bounded recording as well as bounded textures. Whole output pages
        // preserve the existing compositor's tile ownership at window seams.
        for side in [2048u32, 1024, 512, PAGE_SIZE] {
            let plan = Self { side };
            if plan.regions_cached(layers, PixelRect::full(extent),objects).all(|(_, window)| Scene::capture_image_bound(layers, window) <= limit
                && window.size().is_ok_and(|size| size.into_iter().all(|side| side <= maximum_side))) {
                return Ok(Some(plan));
            }
        }
        Err(GpuRasterError::Color(format!(
            "Filter halos exceed the live image pixel limit of {limit} bytes even for one output tile"
        )))
    }

    /// Output regions covering `dirty`, each with the window of input its
    /// filters read.
    pub(super) fn regions(self, scene: SceneView<'_>, dirty: PixelRect) -> impl Iterator<Item = (PixelRect, DocRect)> + '_ {
        self.regions_cached(scene,dirty,None)
    }
    pub(super) fn regions_cached<'a>(self,scene:SceneView<'a>,dirty:PixelRect,objects:Option<&'a object_spatial::SpatialIndex>)->impl Iterator<Item=(PixelRect,DocRect)>+'a {
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
                        (output, images::capture_window_cached(scene, output,objects))
                    })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{EffectInstance, EffectPass, EffectSampling};

    fn background(artwork: &mut layer_core::Artwork, extent: [u32; 2]) {
        use layer_core::authored::*;
        let draft = EffectInstance::new(layer_core::bundled_effect_catalog().get("solid_color").unwrap().program());
        let effect = artwork.effects.insert(PortableId::random(), EffectApplication::new(draft.program, draft.values, extent)).unwrap();
        let occurrence = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), "finite background")).unwrap();
        let stack = artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    }

    fn adjustments(sampling: EffectSampling, count: usize, extent: [u32; 2]) -> layer_core::Document {
        use layer_core::authored::*;
        let mut artwork = Artwork::new(extent).unwrap();
        let mut program = (*layer_core::bundled_effect_catalog().get("exposure").unwrap().program()).clone();
        program.passes = vec![EffectPass { entry: program.entry.clone(), sampling }].into();
        let program=Arc::new(program);
        let stack = artwork.compositions.get(artwork.root).unwrap().result;
        for _ in 0..count {
            let values = EffectInstance::new(program.clone()).values;
            let effect = artwork.effects.insert(PortableId::random(), EffectApplication::new(program.clone(),values,extent)).unwrap();
            let handle = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), "bounded filter")).unwrap();
            artwork.stacks.get_mut(stack).unwrap().entries.push(handle);
        }
        background(&mut artwork, extent);
        layer_core::Document::from_artwork(artwork).unwrap()
    }

    fn image_layers(count:usize,extent:[u32;2])->layer_core::Document {
        use layer_core::authored::*;
        let mut artwork=Artwork::new(extent).unwrap();
        let image=Image::new(layer_core::color::source::rgba8_source([256;2],|_,_|[255;4]));
        let stack=artwork.compositions.get(artwork.root).unwrap().result;
        for _ in 0..count {
            let mut object=ImageObject::new(image.clone());object.interpolation=ImageInterpolation::Nearest;
            object.affine=Affine64([f64::from(extent[0])/256.,0.,0.,f64::from(extent[1])/256.,0.,0.]);
            let object=artwork.objects.insert(PortableId::random(),object).unwrap();
            let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Image")).unwrap();
            artwork.stacks.get_mut(stack).unwrap().entries.push(owner);
        }
        layer_core::Document::from_artwork(artwork).unwrap()
    }

    #[test]
    fn sibling_image_outputs_enter_admission_and_window_when_the_full_planes_exceed_it() {
        let extent=[2048,1536];let mut document=image_layers(3,extent);let limit=8*1024*1024;
        let full=DocRect::from(PixelRect::full(extent));
        assert_eq!(Scene::capture_image_bound(document.scene(),full),3*full.area()*16);
        let plan=Plan::new(document.scene(),extent,limit,16384).unwrap().unwrap();
        let windows=plan.regions(document.scene(),PixelRect::full(extent)).collect::<Vec<_>>();
        assert!(windows.len()>1);assert!(windows.iter().all(|(_,window)|Scene::capture_image_bound(document.scene(),*window)<=limit));
        let first=document.scene().order()[0];document.artwork.occurrences.get_mut(first).unwrap().visible=false;
        let second=document.scene().order()[1];document.artwork.occurrences.get_mut(second).unwrap().offset=[1_000_000,0];
        assert_eq!(Scene::capture_image_bound(document.scene(),full),full.area()*16);
        let raw=layer_core::SceneScope::RawObjects(first);
        assert_eq!(Scene::capture_image_bound(document.scene().with_scope(&raw),full),full.area()*16);
    }

    #[test]
    fn eighty_image_planes_require_their_resident_allowance_even_with_shared_samples() {
        let document=image_layers(80,[256;2]);let full=DocRect::from(PixelRect::full([256;2]));
        assert_eq!(Scene::capture_image_bound(document.scene(),full),80*256*256*16);
        assert!(Plan::new(document.scene(),[256;2],DEFAULT_IMAGE_PIXEL_BYTES,16384).unwrap().is_none());
        assert!(Plan::new(document.scene(),[256;2],64*1024*1024,16384).is_err(),"the smallest output page still needs eighty resident image planes");
        assert_eq!(document.artwork.images().unwrap().len(),1,"sample sharing does not collapse independent layer outputs");
    }

    #[test]
    fn wide_gaussian_keeps_its_finite_clamped_dependency_window() {
        use layer_core::authored::*;
        let extent = [257, 9];
        let mut artwork = Artwork::new(extent).unwrap();
        let mut draft = EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
        draft.set("sigma", layer_core::EffectValue::Number(65536.)).unwrap();
        let effect = artwork.effects.insert(PortableId::random(), EffectApplication::new(draft.program, draft.values, extent)).unwrap();
        let occurrence = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), "wide Gaussian")).unwrap();
        let stack = artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
        background(&mut artwork, extent);
        let document = layer_core::Document::from_artwork(artwork).unwrap();
        let scene = document.scene();
        assert_eq!(Scene::capture_window(scene, PixelRect::full(extent), extent), DocRect::from(PixelRect::full(extent)));
        assert!(Plan::new(scene, extent, DEFAULT_IMAGE_PIXEL_BYTES, 16384).unwrap().is_none());
    }

    #[test]
    fn thin_inputs_window_when_the_budget_fits_but_gpu_dimensions_do_not() {
        let extent = [32768, 1];
        let document = adjustments(EffectSampling::Neighborhood { radius: 19 }, 1, extent);
        let scene = document.scene();
        let window = Scene::capture_window(scene, PixelRect::full(extent), extent);
        assert!(Scene::capture_image_bound(scene, window) < DEFAULT_IMAGE_PIXEL_BYTES);
        let plan = Plan::new(scene, extent, DEFAULT_IMAGE_PIXEL_BYTES, 16384).unwrap().unwrap();
        for (_, input) in plan.regions(scene, PixelRect::full(extent)) {
            assert!(input.size().unwrap().into_iter().all(|side| side <= 16384));
        }
    }

    #[test]
    fn photo_windows_cover_output_and_complete_halos_within_cap() {
        for extent in [[6000, 4000], [8256, 5504], [8192, 7324], [32768, 257]] {
            let document = adjustments(EffectSampling::Neighborhood { radius: 19 }, 5, extent);
            let layers = document.scene();
            let plan = Plan::new(layers, extent, DEFAULT_IMAGE_PIXEL_BYTES, 16384)
                .unwrap()
                .unwrap();
            let mut covered = 0;
            let mut previous = None;
            for (output, window) in plan.regions(layers, PixelRect::full(extent)) {
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
                let effect=artwork.effects.insert(PortableId::random(),EffectApplication::new(draft.program,draft.values,extent)).unwrap();
                let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(effect),"Gaussian")).unwrap();
                artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
            }
            background(&mut artwork, extent);
            let document=layer_core::Document::from_artwork(artwork).unwrap();
            let scene=document.scene();
            let bytes_per_pixel=u64::from(2*count+1)*16;
            let minimum=Plan { side: PAGE_SIZE }.regions(scene, PixelRect::full(extent)).map(|(_, input)| Scene::capture_image_bound(scene, input)).max().unwrap();
            let plan=Plan::new(scene,extent,limit,16384);
            if minimum>limit {assert!(plan.is_err(),"sigma={sigma} depth={count} minimum={minimum} cap={limit}");continue;}
            let plan=plan.unwrap().unwrap();let mut covered=0;let mut halo_pixels=0;let mut windows=0;
            for (output,input) in plan.regions(scene, PixelRect::full(extent)) {
                assert_eq!(input,Scene::capture_window(scene,output,extent));
                let bytes=input.area()*bytes_per_pixel;
                assert!(bytes<=limit,"sigma={sigma} depth={count} actual input={input:?} bytes={bytes} cap={limit}");
                assert_eq!(bytes,Scene::capture_image_bound(scene,input));
                covered+=output.area();halo_pixels+=input.area();windows+=1;
            }
            assert_eq!(covered,u64::from(extent[0])*u64::from(extent[1]));
            if sigma==21.&&count==1&&limit==256<<20 {
                assert_eq!(windows,20,"the admitted61MP composition must use20 large windows");
                let old=Plan{side:1024};
                let old_inputs:u64=old.regions(scene, PixelRect::full(extent)).map(|(_,input)|input.area()).sum();
                assert_eq!(old.regions(scene, PixelRect::full(extent)).count(),70);
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
            Plan::new(global.scene(), extent, DEFAULT_IMAGE_PIXEL_BYTES, 16384)
                .unwrap_err()
                .to_string()
                .contains("Document-wide")
        );
        let handle = global.scene().order()[0];
        global.artwork.occurrences.get_mut(handle).unwrap().visible = false;
        assert!(
            Plan::new(global.scene(), extent, DEFAULT_IMAGE_PIXEL_BYTES, 16384)
                .unwrap()
                .is_none()
        );
        let halo = adjustments(EffectSampling::Neighborhood { radius: 4096 }, 1, extent);
        assert!(
            Plan::new(halo.scene(), extent, DEFAULT_IMAGE_PIXEL_BYTES, 16384)
                .unwrap_err()
                .to_string()
                .contains("halos")
        );
        assert!(
            Plan::new(
                adjustments(EffectSampling::Document, 1, [256, 256]).scene(),
                [256, 256],
                DEFAULT_IMAGE_PIXEL_BYTES,
                16384
            )
            .unwrap()
            .is_none()
        );
    }
}
