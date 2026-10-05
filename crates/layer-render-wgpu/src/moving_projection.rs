use crate::{GpuRasterError, object_image_mips};
use layer_core::{SceneScope, SceneView};
use std::sync::Arc;
pub(crate) type FailedImageTile = (layer_core::authored::PortableId, std::sync::Weak<layer_core::color::source::SourceImage>, [u32;2], layer_core::color::RgbSpace, String);
pub(crate) struct MovingProjection {
    artwork: layer_core::authored::PortableId,
    root: layer_core::authored::CompositionHandle,
    objects: layer_core::authored::Store<layer_core::authored::ImageObject>,
    layers: layer_core::authored::Store<layer_core::authored::ObjectLayer>,
    occurrences: layer_core::authored::Store<layer_core::authored::Occurrence>,
    stacks: layer_core::authored::Store<layer_core::authored::Stack>,
    compositions: layer_core::authored::Store<layer_core::authored::Composition>,
    scope: SceneScope,
    linear: [u32; 4],
    space: layer_core::color::RgbSpace,
    available: bool,
    failures: usize,
    identities: Vec<(layer_core::authored::PortableId, std::sync::Weak<layer_core::color::source::SourceImage>)>,
    pub(crate) requests: Vec<object_image_mips::MovingRequest>,
}
impl MovingProjection {
    fn matches(&self, scene: SceneView<'_>, linear: [u32;4], space: layer_core::color::RgbSpace, available: bool, failures: usize) -> bool {
        let artwork = scene.artwork();
        self.artwork == artwork.id && self.root == artwork.root && self.objects.same_root(&artwork.objects)
            && self.layers.same_root(&artwork.object_layers) && self.occurrences.same_root(&artwork.occurrences)
            && self.stacks.same_root(&artwork.stacks) && self.compositions.same_root(&artwork.compositions)
            && scene.scope().unwrap_or(&SceneScope::All) == &self.scope && self.linear == linear
            && self.space == space && self.available == available && self.failures == failures
    }
    pub(crate) fn storage_bytes(&self) -> u64 {
        (std::mem::size_of::<Self>() + self.identities.capacity()*std::mem::size_of::<(layer_core::authored::PortableId,std::sync::Weak<layer_core::color::source::SourceImage>)>()
            + self.requests.capacity()*std::mem::size_of::<object_image_mips::MovingRequest>()) as u64
    }
    pub(crate) fn update(cache:&mut Option<Self>, scene:SceneView<'_>, mapping:[f32;6], space:layer_core::color::RgbSpace, available:bool, failed:&mut Vec<FailedImageTile>) -> Result<(),GpuRasterError> {
        use layer_core::authored::{Affine64, ImageInterpolation};
        let linear = std::array::from_fn(|i|mapping[i].to_bits());
        if cache.as_ref().is_some_and(|projection|projection.matches(scene,linear,space,available,failed.len())) { return Ok(()); }
        let surface_to_document = Affine64(mapping.map(f64::from)).inverse().ok_or(GpuRasterError::InvalidExtent)?;
        let identities = if cache.as_ref().is_some_and(|projection|projection.artwork == scene.artwork().id && projection.objects.same_root(&scene.artwork().objects)) {
            std::mem::take(&mut cache.as_mut().unwrap().identities)
        } else {
            scene.artwork().objects.iter().map(|(_,_,object)|(object.image.id(),Arc::downgrade(object.image.storage())))
                .collect::<std::collections::BTreeMap<_,_>>().into_iter().collect::<Vec<_>>()
        };
        failed.retain(|(id, source, _, context, _)| *context == space
            && identities.binary_search_by_key(id,|(id,_)|*id).ok().is_some_and(|index|source.ptr_eq(&identities[index].1)));
        let failed_tiles = &*failed;
        let mut requests = std::collections::BTreeMap::<_,object_image_mips::MovingRequest>::new();
        for (id,source,level) in scene.order().iter().copied().filter(|owner| available && scene.visible(*owner))
            .filter_map(|owner| scene.object_layer(owner).map(|layer| (owner, layer)))
            .flat_map(|(owner, layer)| layer.children.iter().filter_map(move |handle| {
                let object = scene.object(*handle)?;
                if !object.visible || object.interpolation == ImageInterpolation::Nearest
                    || failed_tiles.iter().any(|(id,source,_,context,_)| *id == object.image.id() && source.ptr_eq(&Arc::downgrade(object.image.storage())) && *context == space) { return None; }
                let offset = scene.occurrence_offset64(owner);
                let placement = Affine64([1., 0., 0., 1., offset[0], offset[1]]).compose(object.affine);
                let inverse = placement.inverse()?.compose(surface_to_document).0;
                Some((object.image.id(),object.image.storage(),object_image_mips::MovingImages::level(inverse)))
            })) {
            requests.entry(id).and_modify(|request|request.level=request.level.min(level))
                .or_insert_with(||object_image_mips::MovingRequest {id,source:source.clone(),level});
        }
        let artwork = scene.artwork();
        *cache = Some(MovingProjection {artwork:artwork.id,root:artwork.root,objects:artwork.objects.clone(),layers:artwork.object_layers.clone(),
            occurrences:artwork.occurrences.clone(),stacks:artwork.stacks.clone(),compositions:artwork.compositions.clone(),scope:scene.scope().cloned().unwrap_or_default(),
            linear,space,available,failures:failed.len(),identities,requests:requests.into_values().collect()});
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{Document, Edit, RecordChange, authored::*};
    use layer_core::color::RgbSpace;
    #[test]
    fn paint_dabs_and_panning_reuse_image_requests_and_failure_identity_projection() {
        let mut artwork=Artwork::new([256;2]).unwrap();
        let image=Image::new(layer_core::color::source::rgba8_source([8;2],|_,_|[255;4]));
        let handles=(0..1024).map(|_|artwork.objects.insert(PortableId::random(),ImageObject::new(image.clone(),"Image")).unwrap()).collect::<Vec<_>>();
        let layer=artwork.object_layers.insert(PortableId::random(),ObjectLayer {children:handles.clone()}).unwrap();
        let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(layer),"Images")).unwrap();
        let paint=artwork.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:[256;2],raster:Default::default(),base:None,operations:Arc::default()}).unwrap();
        let ink=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),"Ink")).unwrap();
        let stack=artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries=vec![owner,ink];
        let mut doc=Document::from_artwork(artwork).unwrap();
        let mut cache=None;let mut failures=Vec::new();
        let mapping=[0.25,0.,0.,0.25,0.,0.];
        MovingProjection::update(&mut cache,doc.scene(),mapping,RgbSpace::Srgb,true,&mut failures).unwrap();
        let before=cache.as_ref().unwrap();assert_eq!(before.requests.len(),1);assert_eq!(before.identities.len(),1);assert_eq!(before.requests[0].level,2);
        let requests=before.requests.as_ptr();let identities=before.identities.as_ptr();let bytes=before.storage_bytes();
        let mut source=doc.artwork.paint.get(paint).unwrap().clone();source.domain=[257,256];
        doc.apply(Edit::Paint(RecordChange::replace(&doc.artwork.paint,paint,Some(source)).unwrap())).unwrap();
        MovingProjection::update(&mut cache,doc.scene(),[0.25,0.,0.,0.25,120.,-40.],RgbSpace::Srgb,true,&mut failures).unwrap();
        let painted=cache.as_ref().unwrap();assert_eq!(painted.requests.as_ptr(),requests);assert_eq!(painted.identities.as_ptr(),identities);assert_eq!(painted.storage_bytes(),bytes);
        failures.push((image.id(),Arc::downgrade(image.storage()),[0;2],RgbSpace::Srgb,"Unavailable image tile".into()));
        MovingProjection::update(&mut cache,doc.scene(),mapping,RgbSpace::Srgb,true,&mut failures).unwrap();
        let failed=cache.as_ref().unwrap();assert!(failed.requests.is_empty());assert_eq!(failed.identities.as_ptr(),identities);assert_eq!(failures.len(),1);
        MovingProjection::update(&mut cache,doc.scene(),mapping,RgbSpace::DisplayP3,true,&mut failures).unwrap();
        assert!(failures.is_empty());assert_eq!(cache.as_ref().unwrap().requests.len(),1);assert_eq!(cache.as_ref().unwrap().identities.as_ptr(),identities);
        MovingProjection::update(&mut cache,doc.scene(),[0.125,0.,0.,0.125,0.,0.],RgbSpace::DisplayP3,true,&mut failures).unwrap();
        assert_eq!(cache.as_ref().unwrap().requests[0].level,3);
        let mut changed=doc.artwork.object_layers.get(layer).unwrap().clone();changed.children.clear();
        doc.apply(Edit::ObjectLayer(RecordChange::replace(&doc.artwork.object_layers,layer,Some(changed)).unwrap())).unwrap();
        MovingProjection::update(&mut cache,doc.scene(),mapping,RgbSpace::DisplayP3,true,&mut failures).unwrap();
        assert!(cache.as_ref().unwrap().requests.is_empty());
        assert_eq!(cache.as_ref().unwrap().identities.as_ptr(),identities);
    }
}
