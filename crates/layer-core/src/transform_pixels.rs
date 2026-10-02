use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformPixelsRefusal { Target, Locked, Unchanged, Pending }
impl std::fmt::Display for TransformPixelsRefusal {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(match self {Self::Target=>"Select a layer or mask",Self::Locked=>"The layer is locked",Self::Unchanged=>"This target has no transform to apply",Self::Pending=>"Wait for the current edit"})}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransformPixelsScope { Paint { linked_mask: bool }, Mask }
#[derive(Clone, Debug)]
pub struct TransformPixelsPlan {
    pub target: LayerId,
    pub scope: TransformPixelsScope,
    pub input: Project,
    pub output: Layer,
    pub geometry: ImageTransform,
}
impl TransformPixelsPlan {
    pub fn reserved_edit(&self)->Edit {
        let mut output=self.output.clone();let canvas=[self.input.document.width,self.input.document.height];
        let extent=match self.scope {TransformPixelsScope::Paint{..}=>output.local_extent(canvas),TransformPixelsScope::Mask=>output.mask.as_ref().unwrap().local_extent(canvas)};
        let pages=extent.iter().map(|v|u64::from(v.div_ceil(raster::TILE_SIZE))).product::<u64>();
        let reserve=|planes:&[raster::RasterPlane]|raster::RasterRevision::pending_within(pages*planes.iter().map(|p|raster::TileBlob::max_compressed_len(p.descriptor(self.input.document.color)).unwrap() as u64+96).sum::<u64>());
        match self.scope {
            TransformPixelsScope::Paint{linked_mask}=>{output.raster=reserve(&[raster::RasterPlane::Color,raster::RasterPlane::Wetness,raster::RasterPlane::WatercolorWetness]);if linked_mask{output.mask.as_mut().unwrap().raster=reserve(&[raster::RasterPlane::Mask]);}},
            TransformPixelsScope::Mask=>output.mask.as_mut().unwrap().raster=reserve(&[raster::RasterPlane::Mask]),
        }
        Edit::ReplaceLayer(Box::new(output))
    }
}
impl Document {
    pub fn transform_pixels_refusal(&self,id:LayerId)->Option<TransformPixelsRefusal>{
        use TransformPixelsRefusal::*;
        let Some(owner)=self.target_owner(id) else{return Some(Target);};
        if self.is_locked(id){return Some(Locked);}
        let is_mask=owner.id!=id;
        if !is_mask&&owner.kind!=LayerKind::Paint{return Some(Target);}
        if !owner.pending_operations.is_empty()||owner.mask.as_ref().is_some_and(|m|!m.pending_operations.is_empty()){return Some(Pending);}
        let identity=if is_mask {self.layer_geometry(id).as_affine().is_some_and(|m|m==Affine::translation(self.layer_offset(id)))} else {owner.properties.placement.as_affine()==Some(Affine::IDENTITY)};
        identity.then_some(Unchanged)
    }
    pub fn transform_pixels_plan(&self,id:LayerId,interpolation:Interpolation,limits:ProjectLimits)->Result<TransformPixelsPlan,String>{
        if let Some(reason)=self.transform_pixels_refusal(id){return Err(reason.to_string());}
        let original=self.target_owner(id).unwrap();let canvas=[self.width,self.height];
        let active_mask=original.id!=id;let paired=active_mask&&original.mask.as_ref().unwrap().linked&&original.properties.placement.as_affine().is_none();
        let scalar=active_mask&&!paired;
        let scope=if scalar {TransformPixelsScope::Mask}else{TransformPixelsScope::Paint{linked_mask:original.mask.as_ref().is_some_and(|m|m.linked)}};
        let capture_target=if scalar {id}else{original.id};
        let mut geometry=self.layer_geometry(capture_target);geometry.placement.interpolation=interpolation;
        let targets:Vec<_>=match scope {TransformPixelsScope::Mask=>vec![id],TransformPixelsScope::Paint{linked_mask}=>std::iter::once(original.id).chain(original.mask.iter().filter(|_|linked_mask).map(|m|m.id)).collect()};
        let mut bounds=Rect::from_extent(canvas);
        for target in targets {let map=self.layer_geometry(target);let domain=Rect::from_extent(self.target_extent(target)).outset(interpolation.support() as f32);map.validate_for(domain).map_err(|e|e.to_string())?;bounds=bounds.union(map.forward_bounds(domain));}
        if [bounds.min.x,bounds.min.y,bounds.max.x,bounds.max.y].iter().any(|v|!v.is_finite()){return Err("Invalid transform geometry".into());}
        let origin=Point{x:bounds.min.x.floor(),y:bounds.min.y.floor()};let extent=[(bounds.max.x.ceil()-origin.x) as u32,(bounds.max.y.ceil()-origin.y) as u32];
        if extent.contains(&0)||extent.iter().any(|v|*v>limits.dimension){return Err(format!("A target would reach past {} px",limits.dimension));}
        let pages=extent.iter().map(|v|u64::from(v.div_ceil(raster::TILE_SIZE))).product::<u64>();
        let color_bytes=self.color.paint_descriptor().byte_len([raster::TILE_SIZE;2]).unwrap() as u64;let mask_bytes=self.color.coverage_descriptor().byte_len([raster::TILE_SIZE;2]).unwrap() as u64;
        let (planes,bytes)=match scope {TransformPixelsScope::Mask=>(1,mask_bytes),TransformPixelsScope::Paint{linked_mask}=>(3+u64::from(linked_mask),color_bytes+(2+u64::from(linked_mask))*mask_bytes)};
        if pages*planes>limits.tiles as u64||pages*bytes>limits.raster_bytes{return Err("The transformed pixels exceed the drawing's memory limit".into());}
        let mut output=original.clone();let world=self.layer_offset(original.id);let parents=Point{x:world.x-original.properties.offset.x,y:world.y-original.properties.offset.y};
        if scalar {
            let mask=output.mask.as_mut().unwrap();mask.extent=Some(extent);mask.initial=None;mask.raster=Default::default();
            if mask.linked {
                let owner=self.layer_geometry(original.id).projective().ok_or("Apply the layer transform to edit its linked mask")?;
                mask.offset=original.properties.offset;
                mask.placement=Projective::from_affine(Affine::translation(origin)).then(owner.inverse().ok_or("Invalid owner transform")?).ok_or("Invalid mask transform")?;
            } else {mask.offset=Point{x:origin.x-parents.x,y:origin.y-parents.y};mask.placement=Projective::IDENTITY;}
        } else {
            output.properties.offset=Point{x:origin.x-parents.x,y:origin.y-parents.y};output.properties.placement=LayerPlacement::IDENTITY;output.properties.extent=Some(extent);output.source=None;output.raster=Default::default();
            if let Some(mask)=output.mask.as_mut().filter(|m|m.linked){mask.offset=output.properties.offset;mask.placement=Projective::IDENTITY;mask.extent=Some(extent);mask.initial=None;mask.raster=Default::default();}
        }
        let mut input=original.composite_snapshot();input.properties.parent=None;input.properties.extent=Some(original.local_extent(canvas));input.properties.offset=Point{x:world.x-origin.x,y:world.y-origin.y};
        if let Some(mask)=&mut input.mask {let mworld=self.layer_offset(mask.id);mask.offset=Point{x:mworld.x-origin.x,y:mworld.y-origin.y};mask.extent=Some(original.mask.as_ref().unwrap().local_extent(original.local_extent(canvas)));}
        if scalar {input.source=None;input.raster=Default::default();input.pending_operations=Default::default();}
        let mut document=self.clone();document.width=extent[0];document.height=extent[1];document.layers=vec![input];document.active_layer=original.id;document.active_mask=scalar;document.selection=None;document.rulers.clear();document.reference_layers.clear();
        geometry.placement=geometry.placement.post(Projective::from_affine(Affine::translation(Point{x:-origin.x,y:-origin.y}))).ok_or("Invalid capture origin")?;
        Ok(TransformPixelsPlan{target:id,scope,input:Project{document},output,geometry})
    }
}
