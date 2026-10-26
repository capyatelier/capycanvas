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
        let full = Scene::capture_image_bound(layers, PixelRect::full(extent));
        if full <= limit && images::image_sizes_fit(layers,PixelRect::full(extent).into(),maximum_side,objects) {
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
            if plan.regions_cached(layers, PixelRect::full(extent),objects).all(|(output, _window)| Scene::capture_image_bound(layers, output) <= limit
                && images::image_sizes_fit(layers,output.into(),maximum_side,objects)) {
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
        let full=PixelRect::full(extent);
        assert_eq!(Scene::capture_image_bound(document.scene(),full),3*full.area()*16);
        let plan=Plan::new(document.scene(),extent,limit,16384).unwrap().unwrap();
        let windows=plan.regions(document.scene(),PixelRect::full(extent)).collect::<Vec<_>>();
        assert!(windows.len()>1);assert!(windows.iter().all(|(output,_)|Scene::capture_image_bound(document.scene(),*output)<=limit));
        let first=document.scene().order()[0];document.artwork.occurrences.get_mut(first).unwrap().visible=false;
        let second=document.scene().order()[1];document.artwork.occurrences.get_mut(second).unwrap().offset=[1_000_000,0];
        assert_eq!(Scene::capture_image_bound(document.scene(),full),full.area()*16);
        let raw=layer_core::SceneScope::RawObjects(first);
        assert_eq!(Scene::capture_image_bound(document.scene().with_scope(&raw),full),full.area()*16);
    }

    #[test]
    fn eighty_image_planes_require_their_resident_allowance_even_with_shared_samples() {
        let document=image_layers(80,[256;2]);let full=PixelRect::full([256;2]);
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
        assert!(Scene::capture_image_bound(scene, PixelRect::full(extent)) < DEFAULT_IMAGE_PIXEL_BYTES);
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
                assert!(Scene::capture_image_bound(layers, output) <= DEFAULT_IMAGE_PIXEL_BYTES);
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
            let minimum=Plan { side: PAGE_SIZE }.regions(scene, PixelRect::full(extent)).map(|(output, _)| Scene::capture_image_bound(scene, output)).max().unwrap();
            let plan=Plan::new(scene,extent,limit,16384);
            if minimum>limit {assert!(plan.is_err(),"sigma={sigma} depth={count} minimum={minimum} cap={limit}");continue;}
            let plan=plan.unwrap().unwrap();let mut covered=0;let mut halo_pixels=0;let mut windows=0;
            for (output,input) in plan.regions(scene, PixelRect::full(extent)) {
                assert_eq!(input,Scene::capture_window(scene,output,extent));
                let bytes=Scene::capture_image_bound(scene,output);
                assert!(bytes<=limit,"sigma={sigma} depth={count} actual input={input:?} bytes={bytes} cap={limit}");
                assert!(bytes<=input.area()*bytes_per_pixel);
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

    fn clamped_source_document(extent:[u32;2],corner:bool,style:u32)->layer_core::Document {
        use layer_core::authored::*;
        use layer_core::color::source::rgba8_source;
        use layer_core::{Document, DocumentNames, EffectValue};
        let mut artwork = Document::new(PortableId::random(), 65, 33, DocumentNames { paint: "Ink".into(), paper: "Paper".into() }).artwork;
        let root = artwork.root;
        let stack = artwork.compositions.get(root).unwrap().result;
        let ink = artwork.stacks.get(stack).unwrap().entries[0];
        artwork.stacks.get_mut(stack).unwrap().entries = vec![ink];
        let OccurrenceContent::Paint(paint) = artwork.occurrences.get(ink).unwrap().content else { panic!("Paint expected") };
        artwork.paint.get_mut(paint).unwrap().base = Some(PaintBase::new(rgba8_source([65, 33], |x, y| [x as u8, y as u8, 80, 255]).into()));
        if corner { artwork.occurrences.get_mut(ink).unwrap().offset = [i64::from(extent[0] - 65), i64::from(extent[1] - 33)]; }
        if style == 3 { artwork.stacks.get_mut(stack).unwrap().entries.clear(); }
        for sigma in [9., 21., 13.] {
            let mut draft = EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
            draft.set("sigma", EffectValue::Number(sigma)).unwrap();
            let effect = artwork.effects.insert(PortableId::random(), EffectApplication::new(draft.program, draft.values, [65, 33])).unwrap();
            let occurrence = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), "Gaussian")).unwrap();
            if style == 1 { artwork.occurrences.get_mut(occurrence).unwrap().attachment=Attachment::Effect; }
            if style == 2 {
                let coverage=artwork.coverage.insert(PortableId::random(),CoverageSource {domain:[65,33],raster:Default::default(),default_coverage:1.,operations:Arc::default()}).unwrap();
                let item=artwork.occurrences.get_mut(occurrence).unwrap();
                item.mask=Some(MaskUse {source:coverage,enabled:true,linked:false,inverted:false,offset:[17,-9]});
                item.opacity=0.61;item.blend=layer_core::LayerBlend::Multiply;
            }
            artwork.stacks.get_mut(stack).unwrap().entries.insert(0, occurrence);
        }
        artwork.compositions.get_mut(root).unwrap().size = extent;
        Document::from_artwork(artwork).unwrap()
    }

    #[test]
    fn clamped_sparse_inputs_remain_admitted_for_distant_output_windows() {
        for extent in [[6000, 4000], [9504, 6336], [32768, 257]] { for corner in [false, true] { for style in 0..4 {
            let document=clamped_source_document(extent,corner,style);
            let scene = document.scene();
            let plan = Plan::new(scene, extent, DEFAULT_IMAGE_PIXEL_BYTES, 16384).unwrap().unwrap();
            let regions: Vec<_> = plan.regions(scene, PixelRect::full(extent)).collect();
            assert_eq!(regions.iter().map(|(output, _)| output.area()).sum::<u64>(), u64::from(extent[0]) * u64::from(extent[1]));
            assert!(regions.iter().all(|(output, _)| images::image_sizes_fit(scene,(*output).into(),16384,None)
                && Scene::capture_image_bound(scene,*output)<=DEFAULT_IMAGE_PIXEL_BYTES));
            if extent[0]>16384 && style!=3 { assert!(regions.iter().any(|(_,window)|window.size().unwrap()[0]>16384)); }
        }}}
    }
    #[test]
    fn clamped_sparse_windows_preserve_edge_pixels_and_bound_actual_image_storage() {
        fn capture(corner:bool,style:u32,far:bool)->Vec<u8> {
            let extent=[512,257];
            let mut document=clamped_source_document(extent,corner,style);
            if far && style!=3 {
                let paint=document.scene().order().iter().copied().find(|handle|matches!(document.scene().source_target(*handle),Some(SourceTarget::Paint(_)))).unwrap();
                document.artwork.occurrences.get_mut(paint).unwrap().offset[0]+=if corner {20000}else{-20000};
                document=layer_core::Document::from_artwork(document.artwork).unwrap();
            }
            let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
            let output=PixelRect::new(extent[0]/2,80,extent[0]/2+32,104);
            let mut packet=crate::test_support::packet(document.scene(),extent);
            packet.view.width_px=32;packet.view.height_px=24;
            packet.view.document_to_surface=[1.,0.,0.,1.,-(output.min_x() as f32),-80.];
            crate::test_support::submit_ready(&mut r, packet).unwrap();
            let mut scene=Scene::new(&r).unwrap();
            let (texture,_)=create_color_target(&r.device,[32,24],"clamped sparse image oracle");
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|scene.capture_region(r,packet,&texture,output,Output::Artwork(None),encoder));
            let planned=Scene::capture_image_bound(packet.scene,output);
            let actual=scene.image_cache_bytes();
            assert!(actual<=planned,"{extent:?} corner={corner} style={style} actual={actual} planned={planned}");
            assert!(planned<=DEFAULT_IMAGE_PIXEL_BYTES);
            r.uploads.finish(&encoder);encoder.submit(&r.queue);
            let bytes=crate::layer_tests::page_bytes(&r,&texture);
            if style==3 {assert!(bytes.iter().all(|byte|*byte==0));}
            else {assert!(bytes.as_chunks::<16>().0.iter().all(|pixel| f32::from_le_bytes(pixel[8..12].try_into().unwrap())>0.000001 && (f32::from_le_bytes(pixel[12..16].try_into().unwrap())-1.).abs()<2e-5));}
            eprintln!("clamped source {extent:?} corner={corner} style={style} far={far} actual={actual} planned={planned}");
            bytes
        }
        for corner in [false,true] {for style in 0..4 {
            let reference=capture(corner,style,false);
            let distant=capture(corner,style,true);
            assert_eq!(distant,reference,"corner={corner} style={style}: far clamped source-edge pixels");
        }}
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
