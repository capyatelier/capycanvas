use super::{ImageObjectHandle, OccurrenceHandle, OccurrenceContent, Occurrence, RecordChange};
use crate::{Document, DocumentError, Edit};
use crate::color::{ColorProfile, DocumentColor, source::SourceChannels};
use std::{collections::BTreeMap,sync::Arc};

#[derive(Clone, Debug)]
pub struct Image(super::Resource<crate::color::source::SourceImage>);
impl Image {
    pub fn new(samples:Arc<crate::color::source::SourceImage>)->Self {Self(super::Resource::new(samples))}
    pub fn with_id(id:super::PortableId,samples:Arc<crate::color::source::SourceImage>)->Self {Self(super::Resource::with_id(id,samples))}
    pub fn id(&self)->super::PortableId {self.0.id()}
    pub fn storage(&self)->&Arc<crate::color::source::SourceImage> {self.0.storage()}
    pub fn same_owner(&self,other:&Self)->bool {self.0.same_owner(&other.0)}
    pub(crate) fn owner_identity(&self)->usize {self.0.owner_identity()}
    pub(crate) fn owner_metadata_bytes(&self)->usize {self.0.owner_metadata_bytes()}
    pub(crate) fn identity_matches(&self,other:&Self)->bool {
        if self.id()!=other.id() {return false;}
        if Arc::ptr_eq(self.storage(),other.storage()) {return true;}
        if self!=other || self.interpretation.profile!=other.interpretation.profile {return false;}
        self.tiles.iter().zip(&other.tiles).all(|((_,a),(_,b))|a.owner_identity()==b.owner_identity() || a.encoded_fingerprint().is_some_and(|fingerprint|Some(fingerprint)==b.encoded_fingerprint()))
    }
    pub fn import_foreign(images:&[Self])->Result<Vec<Self>,String> {
        let mut originals=BTreeMap::<super::PortableId,&Self>::new();
        let mut imported=BTreeMap::<super::PortableId,Self>::new();
        let mut tiles=BTreeMap::<super::PortableId,Arc<crate::raster::TileBlob>>::new();
        let mut profiles=BTreeMap::<super::PortableId,super::Resource<[u8]>>::new();
        let mut result=Vec::with_capacity(images.len());
        for image in images {
            image.validate()?;
            if tiles.contains_key(&image.id()) || profiles.contains_key(&image.id()) {return Err("Conflicting foreign image namespace".into());}
            if let Some(previous)=originals.insert(image.id(),image) && !previous.identity_matches(image) {return Err("Conflicting foreign image identity".into());}
            let mut samples=(**image).clone();
            for tile in samples.tiles.values_mut() {
                let id=tile.resource_id();
                if originals.contains_key(&id) || profiles.contains_key(&id) {return Err("Conflicting foreign image namespace".into());}
                if let Some(previous)=tiles.get(&id) {
                    if previous.descriptor!=tile.descriptor || (previous.owner_identity()!=tile.owner_identity() && (previous.encoded_fingerprint().is_none() || previous.encoded_fingerprint()!=tile.encoded_fingerprint())) {return Err("Conflicting foreign tile identity".into());}
                    *tile=previous.clone();
                } else {
                    let remapped=Arc::new(tile.alias(super::PortableId::random()));tiles.insert(id,remapped.clone());*tile=remapped;
                }
            }
            if let ColorProfile::Icc(profile)=&mut samples.interpretation.profile {
                let id=profile.id();
                if originals.contains_key(&id) || tiles.contains_key(&id) {return Err("Conflicting foreign image namespace".into());}
                if let Some(previous)=profiles.get(&id) {if previous!=&*profile {return Err("Conflicting foreign profile identity".into());}*profile=previous.clone();}
                else {let remapped=profile.alias(super::PortableId::random());profiles.insert(id,remapped.clone());*profile=remapped;}
            }
            if let Some(previous)=imported.get(&image.id()) {result.push(previous.clone());}
            else {let remapped=Self::new(Arc::new(samples));imported.insert(image.id(),remapped.clone());result.push(remapped);}
        }
        Ok(result)
    }
}
impl std::ops::Deref for Image {type Target=crate::color::source::SourceImage;fn deref(&self)->&Self::Target {&self.0}}
impl AsRef<crate::color::source::SourceImage> for Image {fn as_ref(&self)->&crate::color::source::SourceImage {&self.0}}
impl From<Arc<crate::color::source::SourceImage>> for Image {fn from(source:Arc<crate::color::source::SourceImage>)->Self {Self::new(source)}}
impl PartialEq for Image {fn eq(&self,other:&Self)->bool {self.id()==other.id() && self.0==other.0}}
impl Eq for Image {}

#[derive(Default)]
struct ImageResourceOwners {
    tiles:BTreeMap<super::PortableId,Arc<crate::raster::TileBlob>>,
    profiles:BTreeMap<super::PortableId,super::Resource<[u8]>>,
}
impl ImageResourceOwners {
    fn tile(&mut self,value:&Arc<crate::raster::TileBlob>)->Result<Arc<crate::raster::TileBlob>,String> {
        if let Some(original)=self.tiles.get(&value.resource_id()) {
            if original.descriptor!=value.descriptor || (original.owner_identity()!=value.owner_identity() && original.encoded_fingerprint().is_none_or(|hash|Some(hash)!=value.encoded_fingerprint())) {return Err("Conflicting immutable image tile identity".into());}
            Ok(original.clone())
        }else {self.tiles.insert(value.resource_id(),value.clone());Ok(value.clone())}
    }
    fn profile(&mut self,value:&super::Resource<[u8]>)->Result<super::Resource<[u8]>,String> {
        if let Some(original)=self.profiles.get(&value.id()) {
            if original!=value {return Err("Conflicting immutable image profile identity".into());}
            Ok(original.clone())
        }else {self.profiles.insert(value.id(),value.clone());Ok(value.clone())}
    }
    fn source(&mut self,source:Arc<crate::color::source::SourceImage>)->Result<Arc<crate::color::source::SourceImage>,String> {
        let mut samples=(*source).clone();let mut changed=false;
        for value in samples.tiles.values_mut() {
            let canonical=self.tile(value)?;
            changed|=!Arc::ptr_eq(value,&canonical);*value=canonical;
        }
        if let ColorProfile::Icc(value)=&mut samples.interpretation.profile {
            let canonical=self.profile(value)?;
            changed|=!value.same_owner(&canonical);*value=canonical;
        }
        Ok(if changed {Arc::new(samples)}else {source})
    }
    fn raster(&mut self,root:&crate::raster::RasterRevision)->Result<crate::raster::RasterRevision,String> {
        let Some(data)=root.try_data() else {return Ok(root.clone());};
        let mut data=(*data?).clone();let mut changed=false;
        for tile in data.tiles.values_mut() {
            if let Some(backing)=tile.try_backing() {
                let backing=backing?;let canonical=self.tile(&backing)?;
                if !Arc::ptr_eq(&backing,&canonical) {*tile=crate::raster::RasterTile::backed_shared(canonical);changed=true;}
            }
        }
        Ok(if changed {crate::raster::RasterRevision::backed(data)}else {root.clone()})
    }
    fn artwork(reference:&crate::Artwork)->Result<Self,String> {
        let mut inventory=crate::RootInventory::default();inventory.artwork(reference);
        let mut owners=Self::default();
        for source in inventory.sources {owners.source(source.clone())?;}
        for raster in inventory.rasters {
            if let Some(data)=raster.try_data() {
                for tile in data?.tiles.values() {if let Some(tile)=tile.try_backing() {owners.tile(&tile?)?;}}
            }
        }
        for profile in inventory.profiles {if let ColorProfile::Icc(profile)=profile {owners.profile(profile)?;}}
        Ok(owners)
    }
}
impl crate::Artwork {
    pub fn intern_source_image(&self,source:Arc<crate::color::source::SourceImage>)->Result<Arc<crate::color::source::SourceImage>,String> {
        source.validate()?;
        ImageResourceOwners::artwork(self)?.source(source)
    }
    pub fn intern_images_from(&mut self,reference:&Self)->Result<(),String> {
        let previous=reference.images()?;
        let mut owners=ImageResourceOwners::artwork(reference)?;
        let mut images=BTreeMap::new();
        for (id,image) in self.images()? {
            let source=owners.source(image.storage().clone())?;
            let canonical=if Arc::ptr_eq(&source,image.storage()) {image.clone()}else {Image::with_id(id,source)};
            let canonical=if let Some(original)=previous.get(&id) {
                if !original.identity_matches(&canonical) {return Err("Conflicting immutable image identity".into());}
                (*original).clone()
            }else{canonical};
            images.insert(id,canonical);
        }
        let mut roots=BTreeMap::new();
        for root in self.paint.iter().map(|(_,_,paint)|&paint.raster).chain(self.coverage.iter().map(|(_,_,coverage)|&coverage.raster)) {
            if let std::collections::btree_map::Entry::Vacant(entry)=roots.entry(root.identity()) {entry.insert(owners.raster(root)?);}
        }
        let mut proofs=BTreeMap::new();
        for (handle,_,output) in self.outputs.iter() {
            if let Some(proof)=&output.proof && let ColorProfile::Icc(profile)=&proof.profile {proofs.insert(handle,owners.profile(profile)?);}
        }
        let paint=self.paint.iter().map(|(h,_,_)|h).collect::<Vec<_>>();
        for h in paint {let paint=self.paint.get_mut(h).unwrap();paint.raster=roots[&paint.raster.identity()].clone();if let Some(base)=&mut paint.base {base.image=images[&base.image.id()].clone();}}
        let coverage=self.coverage.iter().map(|(h,_,_)|h).collect::<Vec<_>>();
        for h in coverage {let coverage=self.coverage.get_mut(h).unwrap();coverage.raster=roots[&coverage.raster.identity()].clone();}
        for (handle,profile) in proofs {self.outputs.get_mut(handle).unwrap().proof.as_mut().unwrap().profile=ColorProfile::Icc(profile);}
        let objects=self.objects.iter().map(|(h,_,_)|h).collect::<Vec<_>>();
        for h in objects {let object=self.objects.get_mut(h).unwrap();object.image=images[&object.image.id()].clone();}
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affine64Error { Invalid(&'static str), Unsupported(&'static str) }
impl std::fmt::Display for Affine64Error {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {match self {Self::Invalid(message)|Self::Unsupported(message)=>f.write_str(message)}}}
impl From<Affine64Error> for String {fn from(error:Affine64Error)->Self {error.to_string()}}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Affine64(pub [f64; 6]);
impl Default for Affine64 { fn default()->Self { Self([1.,0.,0.,1.,0.,0.]) } }
impl Affine64 {
    pub fn map(self,p:[f64;2])->[f64;2] {
        let [a,b,c,d,x,y]=self.0; [a*p[0]+c*p[1]+x,b*p[0]+d*p[1]+y]
    }
    pub fn compose(self,inner:Self)->Self {
        let [a,b,c,d,x,y]=self.0;let [e,f,g,h,u,v]=inner.0;
        Self([a*e+c*f,b*e+d*f,a*g+c*h,b*g+d*h,a*u+c*v+x,b*u+d*v+y])
    }
    fn normalized(self)->Result<([f64;4],f64,[f64;2]),Affine64Error> {
        if !self.0.iter().all(|v|v.is_finite()) {return Err(Affine64Error::Invalid("Nonfinite image affine placement"));}
        let [a,b,c,d,_,_]=self.0;let scale=[a.abs().max(b.abs()),c.abs().max(d.abs())];
        if exact_product(a,d)==exact_product(b,c) {return Err(Affine64Error::Invalid("Singular image affine placement"));}
        let linear=[a/scale[0],b/scale[0],c/scale[1],d/scale[1]];
        if linear.iter().zip([a,b,c,d]).any(|(normalized,original)|*normalized==0. && original!=0.) {return Err(Affine64Error::Unsupported("Image affine coefficients exceed numerical support"));}
        let [a,b,c,d]=linear;let product=b*c;let error=b.mul_add(c,-product);let det=a.mul_add(d,-product)-error;
        if det==0. {return Err(Affine64Error::Unsupported("Image affine determinant exceeds numerical support"));}
        Ok((linear,det,scale))
    }
    pub fn inverse(self)->Option<Self> {
        let ([a,b,c,d],det,scale)=self.normalized().ok()?;
        let inverse=[d/det/scale[0],-b/det/scale[1],-c/det/scale[0],a/det/scale[1]];
        let [x,y]=[self.0[4],self.0[5]];
        let result=Self([inverse[0],inverse[1],inverse[2],inverse[3],-inverse[0].mul_add(x,inverse[2]*y),-inverse[1].mul_add(x,inverse[3]*y)]);
        result.0.iter().all(|v|v.is_finite()).then_some(result)
    }
    pub fn validate(self)->Result<(),Affine64Error> {
        self.normalized()?;
        self.inverse().map(|_|()).ok_or(Affine64Error::Unsupported("Image affine inverse exceeds numerical support"))
    }
    pub fn bounds(self,extent:[u32;2])->[[f64;2];2] {
        let points=[[0.,0.],[f64::from(extent[0]),0.],[0.,f64::from(extent[1])],[f64::from(extent[0]),f64::from(extent[1])]].map(|p|self.map(p));
        let mut bounds=[points[0];2];
        for p in points {for axis in 0..2 {bounds[0][axis]=bounds[0][axis].min(p[axis]);bounds[1][axis]=bounds[1][axis].max(p[axis]);}}
        bounds
    }
}
fn exact_product(a:f64,b:f64)->(bool,u128,i32) {
    fn parts(value:f64)->(bool,u64,i32) {
        let bits=value.to_bits();let exponent=((bits>>52)&2047) as i32;let mantissa=bits&((1<<52)-1);
        (bits>>63!=0,if exponent==0 {mantissa}else {mantissa|(1<<52)},if exponent==0 {-1074}else {exponent-1075})
    }
    let (a_sign,a_mantissa,a_exponent)=parts(a);let (b_sign,b_mantissa,b_exponent)=parts(b);
    let product=u128::from(a_mantissa)*u128::from(b_mantissa);
    if product==0 {return (false,0,0);}
    let shift=product.trailing_zeros();(a_sign!=b_sign,product>>shift,a_exponent+b_exponent+shift as i32)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ImageInterpolation { Nearest, #[default] Linear }

pub const MAX_NAME_BYTES: usize = 4096;
pub const MAX_NAME_CHARS: usize = 128;
pub fn bounded_name(name: &str) -> Arc<str> {
    name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_NAME_CHARS).collect::<String>().trim_end().into()
}
#[derive(Clone, Debug, PartialEq)]
pub struct ImageObject {
    pub image:Image,
    pub affine:Affine64,
    pub interpolation:ImageInterpolation,
}
impl ImageObject {
    pub fn new(image:Image)->Self {Self {image,affine:Default::default(),interpolation:Default::default()}}
    pub fn admit_affine(&self)->Result<(),Affine64Error> {
        self.affine.validate()?;
        if self.affine.bounds(self.image.extent).iter().flatten().any(|v|!v.is_finite()) {return Err(Affine64Error::Unsupported("Image affine bounds exceed numerical support"));}
        Ok(())
    }
    pub fn validate(&self)->Result<(),String> {
        self.image.validate()?;self.validate_presentation()
    }
    pub(crate) fn validate_presentation(&self)->Result<(),String> {
        self.admit_affine().map_err(String::from)?;
        Ok(())
    }
    pub(crate) fn same_image(&self,other:&Self)->bool {self.image.id()==other.image.id() && Arc::ptr_eq(self.image.storage(),other.image.storage())}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum PaintBasePolicy { #[default] SourceProfile, WorkingPixels }

#[derive(Clone, Debug, PartialEq)]
pub struct PaintBase { pub image:Image, pub offset:[u32;2], pub policy:PaintBasePolicy }
impl PaintBase {
    pub fn new(image:Image)->Self {Self {image,offset:[0;2],policy:Default::default()}}
    pub fn is_original(&self)->bool {self.policy==PaintBasePolicy::SourceProfile}
    pub fn validate(&self,domain:[u32;2],color:DocumentColor)->Result<(),String> {
        self.image.validate()?;
        if (0..2).any(|axis|self.offset[axis].checked_add(self.image.extent[axis]).is_none_or(|end|end>domain[axis])) {return Err("Paint base is outside its domain".into());}
        if self.policy==PaintBasePolicy::WorkingPixels && (self.image.interpretation.channels!=SourceChannels::Rgba || self.image.interpretation.depth!=color.depth || self.image.interpretation.profile!=ColorProfile::Builtin(color.space) || self.image.interpretation.profile_assumed) {return Err("Working paint base interpretation differs from the document".into());}
        Ok(())
    }
}

impl Document {
    pub fn import_object_layers_edit(&self,objects:Vec<(Arc<str>,ImageObject)>,parent:Option<OccurrenceHandle>,at:usize)->Result<(Vec<OccurrenceHandle>,Edit),DocumentError> {
        let existing=self.artwork.images().map_err(DocumentError::InvalidArtwork)?;
        let local=|image:&Image|existing.get(&image.id()).filter(|known|known.identity_matches(image)).map(|known|(*known).clone());
        let foreign=Image::import_foreign(&objects.iter().filter(|(_,o)|local(&o.image).is_none()).map(|(_,o)|o.image.clone()).collect::<Vec<_>>()).map_err(DocumentError::InvalidArtwork)?;
        let mut foreign=foreign.into_iter();
        let mut candidate=self.clone();let mut edits=Vec::with_capacity(objects.len());let mut handles=Vec::with_capacity(objects.len());
        for (index,(name,mut object)) in objects.into_iter().enumerate() {
            object.image=local(&object.image).unwrap_or_else(||foreign.next().expect("Imported foreign image"));
            let (handle,edit)=candidate.create_object_layer_edit(name,object,parent,at.saturating_add(index))?;
            candidate.apply(edit.clone())?;handles.push(handle);edits.push(edit);
        }
        Ok((handles,Edit::Batch(edits)))
    }
    pub fn create_object_layer_edit(&self,name:impl Into<Arc<str>>,object:ImageObject,parent:Option<OccurrenceHandle>,at:usize)->Result<(OccurrenceHandle,Edit),DocumentError> {
        object.validate().map_err(DocumentError::InvalidArtwork)?;
        let invalid=DocumentError::InvalidLayerOperation;
        let stack=match parent {
            Some(h)=>{if self.is_locked(h) {return Err(DocumentError::ProtectedOccurrence(h));} match self.scene().occurrence(h).map(|o|&o.content) {Some(OccurrenceContent::Stack(s))=>*s,_=>return Err(invalid("Choose a group"))}},
            None=>self.composition().result,
        };
        let mut entries=self.artwork.stacks.get(stack).ok_or(invalid("Missing destination stack"))?.clone();
        let object=RecordChange::insert(&self.artwork.objects,object);
        let (at,attachment)=self.content_insertion(parent,at.min(entries.entries.len()));
        let mut occurrence=Occurrence::new(OccurrenceContent::Objects(object.handle),name);occurrence.attachment=attachment;
        let occurrence=RecordChange::insert(&self.artwork.occurrences,occurrence);let handle=occurrence.handle;
        entries.entries.insert(at,handle);
        let edit=Edit::Batch(vec![Edit::ImageObject(object),Edit::Occurrence(occurrence),Edit::Stack(RecordChange::replace(&self.artwork.stacks,stack,Some(entries))?)]);
        let mut candidate=self.clone();candidate.apply(edit.clone())?;
        Ok((handle,edit))
    }
    pub fn set_image_object_affine_edit(&self,object:ImageObjectHandle,affine:Affine64)->Result<Edit,DocumentError> {
        let owner=self.scene().object_owner(object).ok_or(DocumentError::InvalidLayerOperation("Choose an image object"))?;
        self.editable_object_layer(owner)?;
        let mut value=self.artwork.objects.get(object).unwrap().clone();value.affine=affine;
        value.validate_presentation().map_err(DocumentError::InvalidArtwork)?;
        Ok(Edit::ImageObject(RecordChange::replace(&self.artwork.objects,object,Some(value))?))
    }
    fn editable_object_layer(&self,layer:OccurrenceHandle)->Result<ImageObjectHandle,DocumentError> {
        if self.is_locked(layer) {return Err(DocumentError::ProtectedOccurrence(layer));}
        match self.scene().occurrence(layer).map(|o|&o.content) {Some(OccurrenceContent::Objects(h))=>Ok(*h),_=>Err(DocumentError::InvalidLayerOperation("Choose an object layer"))}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Artwork, ArtworkQuery, ArtworkSource, PaintSource, PortableId, SceneIndex, color::source::rgba8_source};
    fn document()->Document {Document::from_artwork(Artwork::new([1024,1024]).unwrap()).unwrap()}
    fn image()->Image {rgba8_source([7,5],|x,y|[x as u8,y as u8,17,255]).into()}

    #[test]
    fn same_image_presentation_edits_validate_the_changed_objects_and_restore_exactly() {
        let mut doc=document();let shared=image();
        let (layer,edit)=doc.create_object_layer_edit("Original",ImageObject::new(shared.clone()),None,0).unwrap();doc.apply(edit).unwrap();let first=doc.scene().object_handle(layer).unwrap();
        let (second_layer,edit)=doc.create_object_layer_edit("Second",ImageObject::new(shared.clone()),None,1).unwrap();doc.apply(edit).unwrap();let second=doc.scene().object_handle(second_layer).unwrap();
        let before=doc.clone();
        let moved=Edit::Batch(vec![doc.set_image_object_affine_edit(first,Affine64([0.,1.,-1.,0.,3.5,2.])).unwrap(),doc.set_image_object_affine_edit(second,Affine64([2.,0.,0.,2.,-7.,1.])).unwrap()]);
        assert!(moved.presents_same_images(&doc));
        let inverse=doc.apply(moved).unwrap();
        assert_eq!(doc.revision,before.revision+1);
        assert_eq!(doc.scene().object(first).unwrap().affine,Affine64([0.,1.,-1.,0.,3.5,2.]));
        doc.apply(inverse).unwrap();
        assert_eq!(doc.artwork,before.artwork);
        let mut singular=doc.artwork.objects.get(first).unwrap().clone();singular.affine=Affine64([1.,1.,1.,1.,0.,0.]);
        let edit=Edit::ImageObject(RecordChange::replace(&doc.artwork.objects,first,Some(singular)).unwrap());
        assert!(edit.presents_same_images(&doc));
        assert!(doc.apply(edit).is_err());assert_eq!(doc.artwork,before.artwork);
        let mut replaced=doc.artwork.objects.get(first).unwrap().clone();replaced.image=image();
        let edit=Edit::ImageObject(RecordChange::replace(&doc.artwork.objects,first,Some(replaced)).unwrap());
        assert!(!edit.presents_same_images(&doc),"replacing an image keeps whole-document validation");
        doc.apply(edit).unwrap();
    }
    #[test]
    fn image_identity_is_part_of_authored_equality() {
        let original=image();let copy=original.clone();
        let independent=Image::new(original.storage().clone());
        assert_eq!(original,copy);assert!(original.same_owner(&copy));
        assert_ne!(original,independent);assert_eq!(*original,*independent);
        let mut accounting=crate::color::source::SourceAccounting::default();
        assert!(accounting.charge_image(&original)>original.owner_metadata_bytes());
        assert_eq!(accounting.charge_image(&copy),0);
        assert_eq!(accounting.charge_image(&independent),independent.owner_metadata_bytes());
    }
    #[test]
    fn native_image_admission_rejects_resource_identity_collisions() {
        let mut doc=document();let original=image();
        let (layer,edit)=doc.create_object_layer_edit("Original",ImageObject::new(original.clone()),None,0).unwrap();doc.apply(edit).unwrap();let handle=doc.scene().object_handle(layer).unwrap();
        let tile_id=original.tiles.values().next().unwrap().resource_id();
        let mut artwork=doc.artwork.clone();
        artwork.objects.get_mut(handle).unwrap().image=Image::with_id(tile_id,original.storage().clone());
        assert!(Document::from_artwork(artwork).is_err());
        let before=doc.clone();
        let collision=ImageObject::new(Image::with_id(tile_id,original.storage().clone()));
        assert!(doc.create_object_layer_edit("Collision",collision.clone(),None,1).is_err());
        let edit=Edit::ImageObject(RecordChange::replace(&doc.artwork.objects,handle,Some(collision)).unwrap());
        assert!(doc.apply(edit).is_err());assert_eq!(doc,before);
        let profile=crate::authored::Resource::<[u8]>::new(Arc::from([13u8,29,71]));
        let mut samples=(*original).clone();samples.interpretation.profile=ColorProfile::Icc(profile.clone());
        let mut artwork=doc.artwork.clone();artwork.objects.get_mut(handle).unwrap().image=Image::with_id(profile.id(),Arc::new(samples));
        assert!(Document::from_artwork(artwork).is_err());
        let program=crate::effect_catalog::custom_program("solid_color");
        let code_id=program.wgsl.sources().unwrap()[0].id();
        let mut artwork=doc.artwork.clone();
        artwork.effects.insert(PortableId::random(),crate::EffectApplication::new(program.clone(),crate::EffectInstance::new(program).values,[1024;2])).unwrap();
        artwork.objects.get_mut(handle).unwrap().image=Image::with_id(code_id,original.storage().clone());
        assert!(Document::from_artwork(artwork).is_err());
        let paint=crate::operation_test_support::insert_paint(&mut doc,"Paint",0,None);
        let target=doc.scene().source_target(paint).unwrap();let before=doc.clone();
        let tile=original.tiles.values().next().unwrap();
        let crate::SourceTarget::Paint(paint_handle)=target else {unreachable!()};
        for id in [original.id(),doc.artwork.id,doc.artwork.objects.id(handle).unwrap(),doc.artwork.occurrences.id(paint).unwrap(),doc.artwork.paint.id(paint_handle).unwrap()] {
            let revision=crate::raster::RasterRevision::backed(crate::raster::RasterData {
                tiles:[(crate::raster::TileKey {coordinate:[0;2],plane:crate::raster::RasterPlane::Color},crate::raster::RasterTile::backed(tile.alias(id)))].into(),watercolor:None,
            });
            assert!(doc.apply(Edit::SetRaster {target,revision:revision.clone()}).is_err());assert_eq!(doc,before);
            let mut source=doc.artwork.paint.get(paint_handle).unwrap().clone();source.raster=revision;
            assert!(doc.apply(Edit::Paint(RecordChange::replace(&doc.artwork.paint,paint_handle,Some(source)).unwrap())).is_err());assert_eq!(doc,before);
        }
    }
    #[test]
    fn image_placements_preserve_fractional_high_coordinates_and_inverse() {
        let affine=Affine64([0.,-2.,3.,0.,1_000_000_000.125,-2_000_000_000.375]);
        affine.validate().unwrap();let point=[17.25,8.125];
        let restored=affine.inverse().unwrap().map(affine.map(point));
        assert!((0..2).all(|axis|(restored[axis]-point[axis]).abs()<1e-6));
        assert_eq!(serde_json::from_str::<Affine64>(&serde_json::to_string(&affine).unwrap()).unwrap(),affine);
        assert!(Affine64([1.,2.,2.,4.,0.,0.]).validate().is_err());
        assert!(Affine64([1.,0.,0.,1.,f64::NAN,0.]).validate().is_err());
    }
    #[test]
    fn affine_admission_distinguishes_invalid_geometry_from_numerical_limits() {
        for scale in [1e200,1e-200] {
            let affine=Affine64([scale,0.,0.,scale,0.,0.]);affine.validate().unwrap();
            let inverse=affine.inverse().unwrap();assert!((inverse.0[0]*scale-1.).abs()<1e-15);
        }
        assert!(matches!(Affine64([1e-320,0.,0.,1e-320,0.,0.]).validate(),Err(Affine64Error::Unsupported(_))));
        assert!(matches!(Affine64([1e200,0.,0.,0.,0.,0.]).validate(),Err(Affine64Error::Invalid(_))));
        assert!(matches!(Affine64([1e200,1e200,1e200,1e200,0.,0.]).validate(),Err(Affine64Error::Invalid(_))));
        assert!(matches!(Affine64([f64::INFINITY,0.,0.,1.,0.,0.]).validate(),Err(Affine64Error::Invalid(_))));
        assert!(matches!(Affine64([f64::MAX,f64::from_bits(1),f64::MAX,f64::from_bits(2),0.,0.]).validate(),Err(Affine64Error::Unsupported(_))));
        let mut object=ImageObject::new(image());object.affine=Affine64([f64::MAX,0.,0.,1.,0.,0.]);
        assert!(matches!(object.admit_affine(),Err(Affine64Error::Unsupported(_))));
    }
    #[test]
    fn object_only_documents_duplicate_and_undo_without_paint_targets() {
        let mut doc=document();let image=image();
        let (layer,edit)=doc.create_object_layer_edit("Original",ImageObject::new(image.clone()),None,0).unwrap();doc.apply(edit).unwrap();let first=doc.scene().object_handle(layer).unwrap();
        let (second_layer,edit)=doc.create_object_layer_edit("Second",ImageObject::new(image.clone()),None,1).unwrap();doc.apply(edit).unwrap();let second=doc.scene().object_handle(second_layer).unwrap();
        assert!(doc.artwork.paint.is_empty());assert!(doc.scene().source_target(layer).is_none());assert!(doc.scene().targets().next().is_none());
        let changed=Affine64([0.,1.,-1.,0.,99.5,-8.25]);let undo=doc.apply(doc.set_image_object_affine_edit(first,changed).unwrap()).unwrap();
        assert_eq!(doc.scene().object(first).unwrap().affine,changed);assert_eq!(doc.scene().object(second).unwrap().affine,Affine64::default());
        doc.apply(undo).unwrap();assert_eq!(doc.scene().object(first).unwrap().affine,Affine64::default());
        let (edit,copies)=doc.duplicate_layers_edit(&[layer]).unwrap();doc.apply(edit).unwrap();
        let copy=doc.scene().object_handle(copies[0]).unwrap();
        assert_ne!(copy,first);assert!(doc.scene().object(copy).unwrap().image.same_owner(&image));
        let undo=doc.apply(doc.delete_layers_edit(&[layer,second_layer,copies[0]]).unwrap()).unwrap();
        assert!(doc.artwork.objects.is_empty());assert!(doc.artwork.images().unwrap().is_empty());
        doc.apply(undo).unwrap();assert!(doc.scene().object(first).unwrap().image.same_owner(&image));
    }
    #[test]
    fn deleted_image_identity_remains_immutable_while_undo_retains_it() {
        let mut doc=document();let image=image();
        let (layer,edit)=doc.create_object_layer_edit("Original",ImageObject::new(image.clone()),None,0).unwrap();doc.apply(edit).unwrap();
        let mut editor=crate::Editor::new(doc);editor.perform(editor.document().delete_layers_edit(&[layer]).unwrap()).unwrap();

        let changed=Image::with_id(image.id(),rgba8_source([3,2],|_,_|[13,29,71,255]));
        let (_,edit)=editor.document().create_object_layer_edit("Imported",ImageObject::new(changed),None,0).unwrap();
        let before=editor.document().clone();assert!(editor.perform(edit).is_err());assert_eq!(editor.document(),&before);
        let (handles,edit)=editor.document().import_object_layers_edit(vec![("Imported".into(),ImageObject::new(image))],None,0).unwrap();
        editor.perform(edit).unwrap();assert!(editor.document().scene().object_layer(handles[0]).is_some());
    }
    #[test]
    fn undo_only_resource_ids_cannot_be_reused_by_new_images() {
        let mut doc=document();let original=image();
        let (layer,edit)=doc.create_object_layer_edit("Original",ImageObject::new(original.clone()),None,0).unwrap();doc.apply(edit).unwrap();
        let tile_id=original.tiles.values().next().unwrap().resource_id();let mut editor=crate::Editor::new(doc);
        editor.perform(editor.document().delete_layers_edit(&[layer]).unwrap()).unwrap();
        assert!(editor.document().artwork.images().unwrap().is_empty());

        let collision=Image::with_id(tile_id,rgba8_source([3,2],|_,_|[13,29,71,255]));
        let (_,edit)=editor.document().create_object_layer_edit("Imported",ImageObject::new(collision),None,0).unwrap();
        let before=editor.document().clone();let checkpoint=editor.checkpoint();
        assert!(editor.perform(edit).is_err());assert_eq!(editor.document(),&before);assert_eq!(editor.checkpoint(),checkpoint);
        editor.undo().unwrap();assert!(editor.document().artwork.images().unwrap()[&original.id()].same_owner(&original));
    }
    #[test]
    fn pending_bake_inputs_keep_image_identity_immutable_during_direct_edits() {
        let mut captured=document();let original=image();
        let (layer,edit)=captured.create_object_layer_edit("Original",ImageObject::new(original.clone()),None,0).unwrap();captured.apply(edit).unwrap();
        let mut pending=document();
        let row=crate::operation_test_support::insert_paint(&mut pending,"Bake",0,None);
        let OccurrenceContent::Paint(paint)=pending.scene().occurrence(row).unwrap().content else {panic!()};
        pending.artwork.paint.get_mut(paint).unwrap().operations=Arc::new(vec![crate::RasterOperation {
            placement:crate::Affine::IDENTITY,
            coverage:crate::CoverageSnapshot::reveal_all(crate::CoverageHandle::from_index(50),[1024;2],[0;2]),
            kind:crate::RasterOperationKind::Bake {scene:captured.snapshot(),scope:crate::SceneScope::Members(vec![layer].into()),offset:crate::Point::default()},
        }]);

        let changed=Image::with_id(original.id(),rgba8_source(original.extent,|_,_|[71,29,13,255]));
        let before=pending.clone();
        assert!(pending.create_object_layer_edit("Imported",ImageObject::new(changed),None,0).is_err());
        assert_eq!(pending,before);
        let (_,edit)=pending.create_object_layer_edit("Imported",ImageObject::new(original),None,0).unwrap();pending.apply(edit).unwrap();
    }
    #[test]
    fn raw_objects_queries_ignore_layer_properties_and_track_object_edits() {
        let mut doc=document();
        let (layer,edit)=doc.create_object_layer_edit("Original",ImageObject::new(image()),None,0).unwrap();doc.apply(edit).unwrap();let object=doc.scene().object_handle(layer).unwrap();
        let raw=ArtworkQuery::new(&doc,ArtworkSource::Objects(layer));let visible=ArtworkQuery::new(&doc,ArtworkSource::Visible);
        let mut occurrence=doc.scene().occurrence(layer).unwrap().clone();occurrence.opacity=0.2;occurrence.visible=false;
        doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,layer,Some(occurrence)).unwrap())).unwrap();
        assert!(raw.matches_source(&doc));assert!(!visible.matches_source(&doc));
        doc.apply(doc.set_image_object_affine_edit(object,Affine64([1.,0.,0.,1.,0.25,0.5])).unwrap()).unwrap();
        assert!(!raw.matches_source(&doc));
    }
    #[test]
    fn image_interning_validates_and_pools_shared_tile_and_profile_ids() {
        let reference=Artwork::new([16,16]).unwrap();
        let first=image();let mut second=(*first).clone();
        second.tiles=second.tiles.iter().map(|(coordinate,tile)|(*coordinate,Arc::new(tile.alias(tile.resource_id())))).collect();
        let second=Image::new(Arc::new(second));
        let mut incoming=reference.clone();
        let a=incoming.objects.insert(PortableId::random(),ImageObject::new(first.clone())).unwrap();
        let b=incoming.objects.insert(PortableId::random(),ImageObject::new(second)).unwrap();
        incoming.intern_images_from(&reference).unwrap();
        assert!(Arc::ptr_eq(&incoming.objects.get(a).unwrap().image.tiles[&[0,0]],&incoming.objects.get(b).unwrap().image.tiles[&[0,0]]));
        let mut conflicting=(*rgba8_source(first.extent,|_,_|[1,2,3,255])).clone();
        let tile=conflicting.tiles.get_mut(&[0,0]).unwrap();*tile=Arc::new(tile.alias(first.tiles[&[0,0]].resource_id()));
        let mut bad=incoming.clone();bad.objects.get_mut(b).unwrap().image=Image::new(Arc::new(conflicting));
        assert!(bad.intern_images_from(&reference).is_err());
        assert!(reference.clone().intern_images_from(&bad).is_err());
        let profile=crate::authored::Resource::<[u8]>::new(Arc::from([1,2,3]));
        let mut one=(*first).clone();one.interpretation.profile=ColorProfile::Icc(profile.clone());
        let mut two=one.clone();two.interpretation.profile=ColorProfile::Icc(crate::authored::Resource::with_id(profile.id(),Arc::from([3,2,1])));
        let mut bad=reference.clone();
        bad.objects.insert(PortableId::random(),ImageObject::new(Image::new(Arc::new(one)))).unwrap();
        bad.objects.insert(PortableId::random(),ImageObject::new(Image::new(Arc::new(two)))).unwrap();
        assert!(incoming.intern_images_from(&bad).is_err());
        assert!(bad.intern_images_from(&reference).is_err());
    }
    #[test]
    fn source_interning_reuses_raster_backing_and_proof_profiles_without_images() {
        use crate::raster::{RasterTile,RasterData,RasterRevision,TileKey,RasterPlane};
        let mut reference=Artwork::new([16,16]).unwrap();let source=rgba8_source([2,1],|_,_|[17,29,81,255]);
        let tile=source.tiles[&[0,0]].clone();let key=TileKey {plane:RasterPlane::Color,coordinate:[0,0]};
        let paint=reference.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:[16,16],raster:RasterRevision::backed(RasterData {tiles:[(key,RasterTile::backed_shared(tile.clone()))].into(),watercolor:None}),base:None,operations:Default::default()}).unwrap();
        let profile=crate::authored::Resource::<[u8]>::new(Arc::from([1,2,3]));
        reference.outputs.get_mut(reference.default_output).unwrap().proof=Some(crate::color::ProofRecipe::new("Proof".into(),ColorProfile::Icc(profile.clone())));
        let decoded=Arc::new(crate::raster::TileBlob::from_package(tile.resource_id(),tile.descriptor,tile.compressed().unwrap()).unwrap());
        let mut incoming_source=(*source).clone();incoming_source.tiles.insert([0,0],decoded.clone());
        incoming_source.interpretation.profile=ColorProfile::Icc(crate::authored::Resource::with_id(profile.id(),Arc::from([1,2,3])));
        let canonical=reference.intern_source_image(Arc::new(incoming_source.clone())).unwrap();
        assert!(Arc::ptr_eq(&canonical.tiles[&[0,0]],&tile));
        let ColorProfile::Icc(canonical_profile)=&canonical.interpretation.profile else {panic!()};assert!(canonical_profile.same_owner(&profile));
        let mut incoming=reference.clone();incoming.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(RasterData {tiles:[(key,RasterTile::backed_shared(decoded))].into(),watercolor:None});
        incoming.outputs.get_mut(incoming.default_output).unwrap().proof.as_mut().unwrap().profile=incoming_source.interpretation.profile;
        incoming.intern_images_from(&reference).unwrap();
        assert!(Arc::ptr_eq(&incoming.paint.get(paint).unwrap().raster.wait_data().unwrap().tiles[&key].wait_backing().unwrap(),&tile));
        let ColorProfile::Icc(canonical_profile)=&incoming.outputs.get(incoming.default_output).unwrap().proof.as_ref().unwrap().profile else {panic!()};assert!(canonical_profile.same_owner(&profile));
    }
    #[test]
    fn many_objects_and_independent_paint_bindings_share_one_immutable_image() {
        let mut artwork=Artwork::new([1024,1024]).unwrap();let image=image();
        let stack=artwork.compositions.get(artwork.root).unwrap().result;
        for _ in 0..1025 {let object=artwork.objects.insert(PortableId::random(),ImageObject::new(image.clone())).unwrap();let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Image")).unwrap();artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);}
        for offset in [[0,0],[11,13]] {
            let base=PaintBase {image:image.clone(),offset,policy:PaintBasePolicy::SourceProfile};
            let paint=artwork.paint.insert(PortableId::random(),PaintSource { color_mode:Default::default(),domain:[1024;2],base:Some(base),raster:Default::default(),operations:Arc::default()}).unwrap();
            let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),"Paint")).unwrap();
            artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
        }
        assert_eq!(artwork.images().unwrap().len(),1);SceneIndex::build(&artwork).unwrap();Document::from_artwork(artwork).unwrap().validate(Default::default()).unwrap();
    }
    #[test]
    fn drawable_ownership_and_base_containment_fail_atomically() {
        let mut doc=document();let image=image();
        let (layer,edit)=doc.create_object_layer_edit("Original",ImageObject::new(image.clone()),None,0).unwrap();doc.apply(edit).unwrap();let object=doc.scene().object_handle(layer).unwrap();
        let OccurrenceContent::Objects(handle)=doc.scene().occurrence(layer).unwrap().content else {unreachable!()};
        let invalid=Occurrence::new(OccurrenceContent::Objects(handle),"Duplicate owner");let before=doc.clone();
        assert!(doc.apply(Edit::Occurrence(RecordChange::insert(&doc.artwork.occurrences,invalid))).is_err());assert_eq!(doc,before);
        let paint=PaintSource { color_mode:Default::default(),domain:[10,10],base:Some(PaintBase {image,offset:[4,0],policy:PaintBasePolicy::SourceProfile}),raster:Default::default(),operations:Arc::default()};
        assert!(doc.apply(Edit::Paint(RecordChange::insert(&doc.artwork.paint,paint))).is_err());assert_eq!(doc,before);
        let mut occurrence=doc.scene().occurrence(layer).unwrap().clone();occurrence.offset=[i64::MAX,0];
        assert!(doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,layer,Some(occurrence)).unwrap())).is_err());assert_eq!(doc,before);
        let mut changed=doc.scene().object(object).unwrap().clone();let mut samples=(*changed.image).clone();samples.interpretation.profile=ColorProfile::Builtin(crate::color::RgbSpace::DisplayP3);
        changed.image=Image::with_id(changed.image.id(),Arc::new(samples));
        assert!(doc.apply(Edit::ImageObject(RecordChange::replace(&doc.artwork.objects,object,Some(changed)).unwrap())).is_err());assert_eq!(doc,before);
        let mut changed=doc.scene().object(object).unwrap().clone();let mut samples=(*changed.image).clone();
        let different=rgba8_source(samples.extent,|_,_|[13,29,71,255]);
        for (position,tile) in &mut samples.tiles {*tile=Arc::new(different.tiles[position].alias(tile.resource_id()));}
        changed.image=Image::with_id(changed.image.id(),Arc::new(samples));
        assert_eq!(changed.image,doc.scene().object(object).unwrap().image);
        assert!(doc.apply(Edit::ImageObject(RecordChange::replace(&doc.artwork.objects,object,Some(changed)).unwrap())).is_err());assert_eq!(doc,before);
    }
    #[test]
    fn working_pixel_policy_is_binding_local() {
        let image=image();let source=PaintBase::new(image.clone());let working=PaintBase {policy:PaintBasePolicy::WorkingPixels,..source.clone()};
        source.validate([8,8],Default::default()).unwrap();working.validate([8,8],Default::default()).unwrap();
        let offset=PaintBase {offset:[13,29],..source.clone()};offset.validate([20,34],Default::default()).unwrap();
        assert!(offset.validate([19,34],Default::default()).is_err());
        assert!(PaintBase {offset:[u32::MAX,0],..source.clone()}.validate([u32::MAX;2],Default::default()).is_err());
        let mut color=DocumentColor::default();color.depth=crate::color::SampleDepth::U16;
        source.validate([8,8],color).unwrap();assert!(working.validate([8,8],color).is_err());
        assert!(source.image.same_owner(&working.image));assert!(source.is_original());assert!(!working.is_original());
    }
    #[test]
    fn native_image_admission_rejects_conflicting_resource_kinds_payloads_and_owners() {
        let mut doc=document();let first=image();
        let (_,edit)=doc.create_object_layer_edit("Original",ImageObject::new(first.clone()),None,0).unwrap();doc.apply(edit).unwrap();
        let tile=first.tiles[&[0,0]].clone();
        let mut cross_kind=(*first).clone();cross_kind.interpretation.profile=ColorProfile::Icc(crate::authored::Resource::with_id(tile.resource_id(),Arc::from([1,2,3])));
        let mut payload=(*rgba8_source(first.extent,|_,_|[71,29,13,255])).clone();
        payload.tiles.insert([0,0],Arc::new(payload.tiles[&[0,0]].alias(tile.resource_id())));
        let mut owner=(*first).clone();owner.tiles.insert([0,0],Arc::new(crate::raster::TileBlob::from_package(tile.resource_id(),tile.descriptor,tile.compressed().unwrap()).unwrap()));
        for source in [cross_kind,payload,owner] {
            let image=Image::new(Arc::new(source));let before=doc.clone();
            assert!(doc.create_object_layer_edit("Imported",ImageObject::new(image.clone()),None,0).is_err());assert_eq!(doc,before);
            let mut artwork=doc.artwork.clone();let handle=artwork.objects.insert(PortableId::random(),ImageObject::new(image)).unwrap();
            artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Invalid")).unwrap();
            assert!(Document::from_artwork(artwork).is_err());
        }
    }
    #[test]
    fn retained_tile_identity_rejects_new_payload_before_editor_commit() {
        let mut doc=document();let first=image();
        let (layer,edit)=doc.create_object_layer_edit("Original",ImageObject::new(first.clone()),None,0).unwrap();doc.apply(edit).unwrap();
        let paint=RecordChange::insert(&doc.artwork.paint,PaintSource {color_mode:Default::default(),domain:[1024;2],base:None,raster:Default::default(),operations:Default::default()});
        let target=crate::SourceTarget::Paint(paint.handle);doc.apply(Edit::Paint(paint)).unwrap();
        let mut editor=crate::Editor::new(doc);editor.perform(editor.document().delete_layers_edit(&[layer]).unwrap()).unwrap();

        let mut source=(*rgba8_source(first.extent,|_,_|[71,29,13,255])).clone();
        source.tiles.insert([0,0],Arc::new(source.tiles[&[0,0]].alias(first.tiles[&[0,0]].resource_id())));
        let (_,edit)=editor.document().create_object_layer_edit("Imported",ImageObject::new(Image::new(Arc::new(source.clone()))),None,0).unwrap();
        let before=editor.document().clone();let checkpoint=editor.checkpoint();
        assert!(editor.perform(edit).is_err());assert_eq!(editor.document(),&before);assert_eq!(editor.checkpoint(),checkpoint);
        let revision=crate::raster::RasterRevision::backed(crate::raster::RasterData {tiles:[(crate::raster::TileKey {plane:crate::raster::RasterPlane::Color,coordinate:[0,0]},crate::raster::RasterTile::backed_shared(source.tiles[&[0,0]].clone()))].into(),watercolor:None});
        let edit=Edit::SetRaster {target,revision:revision.clone()};let mut candidate=before.clone();candidate.apply(edit.clone()).unwrap();
        assert!(editor.preview(edit.clone()).is_err());assert_eq!(editor.document(),&before);assert_eq!(editor.checkpoint(),checkpoint);
        assert!(editor.perform(edit).is_err());assert_eq!(editor.document(),&before);assert_eq!(editor.checkpoint(),checkpoint);
        assert!(editor.amend_raster(target,revision).is_err());assert_eq!(editor.document(),&before);assert_eq!(editor.checkpoint(),checkpoint);
        editor.undo().unwrap();assert!(editor.document().artwork.images().unwrap()[&first.id()].same_owner(&first));
    }
    #[test]
    fn foreign_import_rejects_cross_kind_identity_collisions_before_remapping() {
        let doc=document();let source=image();
        let tile_id=source.tiles[&[0,0]].resource_id();
        let image_tile=Image::with_id(tile_id,source.storage().clone());
        let mut samples=(*source).clone();samples.interpretation.profile=ColorProfile::Icc(crate::authored::Resource::with_id(tile_id,Arc::from([1,2,3])));
        let tile_profile=Image::new(Arc::new(samples));
        for invalid in [image_tile,tile_profile] {
            let before=doc.clone();
            assert!(Image::import_foreign(&[invalid.clone()]).is_err());
            assert!(doc.import_object_layers_edit(vec![("Invalid".into(),ImageObject::new(invalid))],None,0).is_err());
            assert_eq!(doc,before);
        }
        let image_tile=Image::with_id(tile_id,rgba8_source([2,1],|_,_|[17,29,81,255]));
        for batch in [vec![source.clone(),image_tile.clone()],vec![image_tile,source]] {assert!(Image::import_foreign(&batch).is_err());}
    }
    #[test]
    fn foreign_import_remaps_dependency_ids_and_preserves_batch_sharing() {
        let mut doc=document();let current=image();
        let (_,edit)=doc.create_object_layer_edit("Original",ImageObject::new(current.clone()),None,0).unwrap();doc.apply(edit).unwrap();
        let foreign=Image::with_id(current.id(),rgba8_source([7,5],|_,_|[71,29,13,255]));
        let tile=foreign.tiles.values().next().unwrap();let foreign_tile_id=tile.resource_id();let foreign_tile_owner=tile.owner_identity();
        let objects=vec![("One".into(),ImageObject::new(foreign.clone())),("Two".into(),ImageObject::new(foreign.clone()))];
        let (handles,edit)=doc.import_object_layers_edit(objects,None,0).unwrap();let undo=doc.apply(edit).unwrap();
        let first=&doc.scene().object_layer(handles[0]).unwrap().image;let second=&doc.scene().object_layer(handles[1]).unwrap().image;
        assert_ne!(first.id(),foreign.id());assert!(first.same_owner(second));assert_ne!(first,&current);
        let imported_tile=first.tiles.values().next().unwrap();assert_ne!(imported_tile.resource_id(),foreign_tile_id);assert_eq!(imported_tile.owner_identity(),foreign_tile_owner);
        assert_eq!(imported_tile.decode().unwrap(),tile.decode().unwrap());
        doc.apply(undo).unwrap();assert!(doc.artwork.images().unwrap().values().any(|image|image.same_owner(&current)));
        let conflict=Image::with_id(foreign.id(),rgba8_source([1,1],|_,_|[0;4]));assert!(Image::import_foreign(&[foreign,conflict]).is_err());
    }
    #[test]
    fn shared_image_and_proof_profile_counts_once_at_asset_limit() {
        let mut doc=document();
        let profile=ColorProfile::Icc(vec![17;512].into());let mut source=(*rgba8_source([1,1],|_,_|[0;4])).clone();source.interpretation.profile=profile.clone();
        let image=Image::new(Arc::new(source));let mut accounting=crate::color::source::SourceAccounting::default();let bytes=accounting.charge_image(&image) as u64;
        let (_,edit)=doc.create_object_layer_edit("Original",ImageObject::new(image),None,0).unwrap();doc.apply(edit).unwrap();
        doc.artwork.outputs.get_mut(doc.artwork.default_output).unwrap().proof=Some(crate::color::ProofRecipe::new("Proof".into(),profile));
        doc.admit(crate::ProjectLimits {asset_bytes:bytes,..Default::default()}).unwrap();
        assert!(doc.admit(crate::ProjectLimits {asset_bytes:bytes-1,..Default::default()}).is_err());
    }
}
