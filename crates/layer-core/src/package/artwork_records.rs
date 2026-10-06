use super::{effect_records, selection_records, manifest::Manifest, resources::{ResourceInventory, ResourceReader, reference, reference_id}, values::{self as v, DecodeError, DecodeResult}};
use super::RASTER_TILE_SIZE as TILE_SIZE;
use crate::{authored::*, color::{DocumentColor, hdr::SdrRendition, source::{SourceImage, SourceChannels, SourceInterpretation}},
    raster::{RasterData, RasterRevision, RasterPlane, RasterWatercolor, TileKey},
    BlendSpace, LayerBlend, PhotoMetadata, RulerGeometry};
use serde_json::{Map, Value, json};
use std::{collections::{BTreeMap, BTreeSet}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

fn record(id:PortableId, kind:&str, data:Value)->Value {
    let kind=super::registry::descriptor(kind).expect("Registered authored record").type_id;
    json!({"id":id,"type":kind,"data":data})
}
fn record_fields<'a>(kind:&str,value:&'a Value)->DecodeResult<&'a Map<String,Value>> {
    fields(value,super::registry::descriptor(kind).ok_or("Missing record adapter")?.data_fields)
}
fn endpoint(id:PortableId)->Value {json!({"object":reference(id),"port":"color"})}
fn id<T>(store:&Store<T>, handle:Handle<T>)->Result<PortableId,String> {
    if store.get(handle).is_none() {return Err("Missing authored object".into());}
    store.id(handle).ok_or_else(||"Missing authored identity".into())
}
fn handle<T>(store:&Store<T>, value:&Value)->DecodeResult<Handle<T>> {
    store.allocated(reference_id(value)?).ok_or_else(||"Reference has the wrong object type".into())
}
fn endpoint_handle<T>(store:&Store<T>, value:&Value)->DecodeResult<Handle<T>> {
    let fields=v::object(value,&["object","port"])?;
    let port=v::string(v::required(fields,"port")?)?;
    if port!="color" {return Err(DecodeError::Unsupported(format!("Unknown evaluation port {port}")));}
    handle(store,v::required(fields,"object")?)
}
fn fields<'a>(value:&'a Value, allowed:&[&str])->DecodeResult<&'a Map<String,Value>> {v::object(value,allowed)}
fn list(value:&Value)->DecodeResult<&[Value]> {value.as_array().map(Vec::as_slice).ok_or_else(||"Expected an array".into())}
fn bool_field(data:&Map<String,Value>,key:&str,default:bool)->DecodeResult<bool> {data.get(key).map(v::boolean).transpose().map(|value|value.unwrap_or(default))}
fn float_field(data:&Map<String,Value>,key:&str,default:f32)->DecodeResult<f32> {data.get(key).map(v::finite_f32).transpose().map(|value|value.unwrap_or(default))}
fn name(data:&Map<String,Value>)->DecodeResult<Arc<str>> {Ok(data.get("name").map(v::string).transpose()?.unwrap_or("").into())}
fn set_bool(data:&mut Map<String,Value>,key:&str,value:bool,default:bool) {if value!=default {data.insert(key.into(),Value::Bool(value));}}
fn set_float(data:&mut Map<String,Value>,key:&str,value:f32,default:f32)->Result<(),String> {
    if !value.is_finite() {return Err("Nonfinite authored value".into());}
    if value.to_bits()!=default.to_bits() {data.insert(key.into(),Value::from(f64::from(value)));} Ok(())
}
fn set_name(data:&mut Map<String,Value>,value:&str) {if !value.is_empty() {data.insert("name".into(),value.into());}}
fn optional_object(data:&mut Map<String,Value>,key:&str,value:Value) {if value.as_object().is_none_or(|v|!v.is_empty()) {data.insert(key.into(),value);}}
fn dimension(value:&Value,reader:&ResourceReader<'_>)->DecodeResult<[u32;2]> {
    let size=v::parse_domain(value)?;
    if size.iter().any(|n|*n>crate::MAX_EXTENT || *n>reader.limits.dimension) {return Err(DecodeError::Unsupported("Source exceeds dimension admission".into()));}
    Ok(size)
}
fn plane_name(plane:RasterPlane)->&'static str {match plane {RasterPlane::Color=>"color",RasterPlane::Mask=>"mask",RasterPlane::WatercolorWetness=>"watercolor_wetness"}}
fn parse_plane(value:&Value)->DecodeResult<RasterPlane> {Ok(match v::string(value)? {"color"=>RasterPlane::Color,"mask"=>RasterPlane::Mask,"watercolor_wetness"=>RasterPlane::WatercolorWetness,name=>return Err(DecodeError::Unsupported(format!("Unknown tile plane {name}")))})}
fn coordinate(value:&Value)->DecodeResult<[u32;2]> {let pair=v::array(value,2)?; Ok([v::u32_value(&pair[0])?,v::u32_value(&pair[1])?])}
fn material(value:&Value)->DecodeResult<Option<RasterWatercolor>> {
    let data=fields(value,&["watercolor"])?;
    data.get("watercolor").map(|value| {let data=fields(value,&["wet_edge","burnt_edge","edge_width"])?;
        let material=RasterWatercolor {wet_edge:v::finite_f32(v::required(data,"wet_edge")?)?,burnt_edge:v::finite_f32(v::required(data,"burnt_edge")?)?,edge_width:v::finite_f32(v::required(data,"edge_width")?)?};
        if material.edge_width<=0. {return Err("Watercolor edge width must be positive".into());}
        if !(1. ..=16.).contains(&material.edge_width) {return Err(DecodeError::Unsupported("Watercolor edge width exceeds evaluator range".into()));}
        Ok(material)}).transpose()
}
fn encode_raster(raster:&RasterRevision, domain:[u32;2], mask:bool,color:DocumentColor,mode:crate::color::LayerColorMode,resources:&mut ResourceInventory,cancel:&AtomicBool)->Result<Map<String,Value>,String> {
    let raster=raster.wait_data_cancellable(cancel)?; raster.validate_index_mode(domain,mask,color,mode)?;
    let RasterData {tiles:raster_tiles,watercolor}=raster.as_ref();
    let mut data=Map::new();
    if !raster_tiles.is_empty() {
        let mut tiles=Vec::new();
        for (key,tile) in raster_tiles {
            if cancel.load(Ordering::Relaxed) {return Err("Package operation cancelled".into());}
            let backing=tile.wait_backing_cancellable(cancel)?;
            if backing.descriptor!=key.plane.descriptor_for(color,mode) {return Err("Wrong raster tile interpretation".into());}
            tiles.push(json!({"coordinate":key.coordinate,"plane":plane_name(key.plane),"resource":resources.tile(backing)?}));
        }
        data.insert("tiles".into(),Value::Array(tiles));
    }
    if let Some(RasterWatercolor {wet_edge,burnt_edge,edge_width})=watercolor {
        data.insert("material".into(),json!({"watercolor":{"wet_edge":f64::from(*wet_edge),"burnt_edge":f64::from(*burnt_edge),"edge_width":f64::from(*edge_width)}}));
    }
    data.insert("domain".into(),v::encode_domain(domain)?); Ok(data)
}
fn decode_raster(data:&Map<String,Value>,domain:[u32;2],mask:bool,color:DocumentColor,mode:crate::color::LayerColorMode,reader:&mut ResourceReader<'_>)->DecodeResult<RasterRevision> {
    let mut raster=RasterData {tiles:BTreeMap::new(),watercolor:data.get("material").map(material).transpose()?.flatten()};
    if let Some(tiles)=data.get("tiles") {
        let tiles=list(tiles)?;
        if tiles.len()>reader.limits.tiles {return Err(DecodeError::Unsupported("Too many raster tiles".into()));}
        for value in tiles {
            let tile=fields(value,&["coordinate","plane","resource"])?;
            let key=TileKey {coordinate:coordinate(v::required(tile,"coordinate")?)?,plane:parse_plane(v::required(tile,"plane")?)?};
            if raster.tiles.contains_key(&key) || key.coordinate[0]>=domain[0].div_ceil(TILE_SIZE) || key.coordinate[1]>=domain[1].div_ceil(TILE_SIZE) || mask!=(key.plane==RasterPlane::Mask) {return Err("Invalid raster tile index".into());}
            let resource=v::required(tile,"resource")?;
            raster.tiles.insert(key,reader.raster_tile(resource)?);
        }
    }
    raster.validate_mode(domain,mask,color,mode)?; Ok(RasterRevision::backed(raster))
}
fn encode_image_samples(source:&SourceImage,resources:&mut ResourceInventory)->Result<Value,String> {
    source.validate()?;
    let channels=match source.interpretation.channels {SourceChannels::Gray=>"gray",SourceChannels::GrayAlpha=>"gray_alpha",SourceChannels::Rgb=>"rgb",SourceChannels::Rgba=>"rgba",SourceChannels::Cmyk=>"cmyk"};
    let mut interpretation=json!({"channels":channels,"depth":v::encode_depth(source.interpretation.depth),"profile":resources.profile(&source.interpretation.profile)?});
    if source.interpretation.profile_assumed {interpretation["profile_assumed"]=true.into();}
    let mut data=json!({"extent":v::encode_size(source.extent)?,"interpretation":interpretation,"tiles":source.tiles.iter().map(|(coordinate,tile)|Ok(json!({"coordinate":coordinate,"resource":resources.tile(tile.clone())?}))).collect::<Result<Vec<_>,String>>()?});
    if let Some(resolution)=source.resolution {data["resolution"]=v::encode_resolution(resolution)?;}
    Ok(data)
}
fn decode_image(identity:PortableId,value:&Value,reader:&mut ResourceReader<'_>)->DecodeResult<Image> {
    if let Some((descriptor,image))=reader.images.get(&identity) {
        if descriptor!=value {return Err("Conflicting immutable image identity".into());}
        return Ok(image.clone());
    }
    let data=record_fields("capy.image/1",value)?;
    let extent=v::parse_size(v::required(data,"extent")?)?;
    if extent.iter().any(|n|*n>reader.limits.dimension) {return Err(DecodeError::Unsupported("Original exceeds dimension admission".into()));}
    let interpretation=fields(v::required(data,"interpretation")?,&["channels","depth","profile","profile_assumed"])?;
    let channels=match v::string(v::required(interpretation,"channels")?)? {"gray"=>SourceChannels::Gray,"gray_alpha"=>SourceChannels::GrayAlpha,"rgb"=>SourceChannels::Rgb,"rgba"=>SourceChannels::Rgba,"cmyk"=>SourceChannels::Cmyk,name=>return Err(DecodeError::Unsupported(format!("Unknown source channels {name}")))};
    let interpretation=SourceInterpretation {channels,depth:v::parse_depth(v::required(interpretation,"depth")?)?,profile:reader.profile(v::required(interpretation,"profile")?)?,profile_assumed:bool_field(interpretation,"profile_assumed",false)?};
    if interpretation.depth.is_float() && (!matches!(interpretation.profile,crate::color::ColorProfile::Builtin(_)) || !matches!(channels,SourceChannels::Rgb|SourceChannels::Rgba)) {
        return Err(DecodeError::Unsupported("HDR source interpretation is unsupported".into()));
    }
    let mut tiles=BTreeMap::new(); let records=list(v::required(data,"tiles")?)?;
    if records.len()>reader.limits.tiles {return Err(DecodeError::Unsupported("Too many original tiles".into()));}
    for value in records {
        let data=fields(value,&["coordinate","resource"])?; let coordinate=coordinate(v::required(data,"coordinate")?)?;
        if tiles.contains_key(&coordinate) || coordinate[0]>=extent[0].div_ceil(TILE_SIZE) || coordinate[1]>=extent[1].div_ceil(TILE_SIZE) {return Err("Invalid original tile index".into());}
        let tile=reader.tile(v::required(data,"resource")?)?;
        tiles.insert(coordinate,tile);
    }
    let source=SourceImage {extent,interpretation,tiles,resolution:data.get("resolution").map(v::parse_resolution).transpose()?}; source.validate()?; let image=Image::with_id(identity,Arc::new(source)); reader.images.insert(identity,(value.clone(),image.clone())); Ok(image)
}
fn encode_image(image:&Image,resources:&mut ResourceInventory)->Result<Value,String> {
    let data=encode_image_samples(image,resources)?;
    let value=record(image.id(),"capy.image/1",data);
    resources.image(image.id(),value)
}
fn image_reference(value:&Value,reader:&mut ResourceReader<'_>)->DecodeResult<Image> {
    let identity=reference_id(value)?;
    if let Some((_,image))=reader.images.get(&identity) {return Ok(image.clone());}
    let record=reader.manifest.objects.get(&identity).ok_or("Missing immutable image")?;
    if record["type"]!="capy.image/1" {return Err("Reference has the wrong object type".into());}
    decode_image(identity,&record["data"].clone(),reader)
}
fn finite_double(value:&Value)->DecodeResult<f64> {value.as_f64().filter(|value|value.is_finite()).ok_or_else(||"Expected finite double".into())}
fn affine_error(error:Affine64Error)->DecodeError {match error {Affine64Error::Invalid(reason)=>DecodeError::Invalid(reason.into()),Affine64Error::Unsupported(reason)=>DecodeError::Unsupported(reason.into())}}
fn decode_affine(value:&Value)->DecodeResult<Affine64> {
    let values=v::array(value,6)?;let mut affine=[0.;6];for (slot,value) in affine.iter_mut().zip(values) {*slot=finite_double(value)?;}
    let affine=Affine64(affine);affine.validate().map_err(affine_error)?;Ok(affine)
}
fn encode_offset(offset:[i64;2])->Value {json!(offset.map(|value|value.to_string()))}
fn decode_offset(value:&Value)->DecodeResult<[i64;2]> {
    let pair=v::array(value,2)?;
    Ok([super::manifest::decimal_i64(&pair[0])?,super::manifest::decimal_i64(&pair[1])?])
}
fn encode_guides(guides:&Guides)->Result<Value,String> {
    if guides.rulers.len()>crate::rulers::MAX_RULERS {return Err("Too many rulers".into());}
    let mut seen=BTreeSet::new(); let mut rulers=Vec::new();
    for (id,geometry) in &guides.rulers {
        if !seen.insert(*id) {return Err("Duplicate ruler identity".into());} geometry.validate().map_err(|e|e.to_string())?;
        let geometry=match *geometry {RulerGeometry::Straight {start,end}=>json!({"kind":"straight","start":v::encode_point(start)?,"end":v::encode_point(end)?}),RulerGeometry::Parallel {start,end}=>json!({"kind":"parallel","start":v::encode_point(start)?,"end":v::encode_point(end)?}),RulerGeometry::Radial {center}=>json!({"kind":"radial","center":v::encode_point(center)?})};
        rulers.push(json!({"id":id,"geometry":geometry}));
    }
    Ok(if rulers.is_empty() {json!({})} else {json!({"rulers":rulers})})
}
fn decode_guides(value:&Value)->DecodeResult<Guides> {
    let data=fields(value,&["rulers"])?; let mut rulers=Vec::new(); let mut seen=BTreeSet::new();
    if let Some(value)=data.get("rulers") {let records=list(value)?;if records.len()>crate::rulers::MAX_RULERS {return Err(DecodeError::Unsupported("Too many rulers".into()));}for ruler in records {
        let ruler=fields(ruler,&["id","geometry"])?; let id=v::string(v::required(ruler,"id")?)?.parse::<PortableId>()?;
        if !seen.insert(id) {return Err("Duplicate ruler identity".into());}
        let geometry=v::required(ruler,"geometry")?; let kind=v::string(v::required(geometry.as_object().ok_or("Expected ruler geometry")?,"kind")?)?;
        let geometry=match kind {"straight"|"parallel"=> {let data=fields(geometry,&["kind","start","end"])?; let start=v::parse_point(v::required(data,"start")?)?; let end=v::parse_point(v::required(data,"end")?)?; if kind=="straight" {RulerGeometry::Straight {start,end}} else {RulerGeometry::Parallel {start,end}}},"radial"=> {let data=fields(geometry,&["kind","center"])?; RulerGeometry::Radial {center:v::parse_point(v::required(data,"center")?)?}},name=>return Err(DecodeError::Unsupported(format!("Unknown ruler {name}")))};
        let (start,end)=geometry.handles();
        if end==Some(start) {return Err("Ruler endpoints must be distinct".into());}
        geometry.validate().map_err(|e|DecodeError::Unsupported(e.to_string()))?; rulers.push((id,geometry));
    }} Ok(Guides {rulers})
}

pub(crate) fn encode_change(art:&Artwork,edit:&crate::Edit,resources:&mut ResourceInventory,cancel:&AtomicBool)->Result<Value,String> {
    let canvas=art.compositions.get(art.root).ok_or("Missing root composition")?;
    match edit {
        crate::Edit::Composition(change)=>{let identity=change.id;let c=change.value.as_ref().ok_or("Cannot encode removed record")?;
            if c.color.depth.is_float() && c.blend!=BlendSpace::Linear {return Err("Floating compositions require linear light blending".into());}
        let mut data=json!({"size":v::encode_size(c.size)?,"result":endpoint(id(&art.stacks,c.result)?)}).as_object().unwrap().clone();
        optional_object(&mut data,"color",v::encode_document_color(c.color)?);
        if c.blend!=BlendSpace::Linear {data.insert("blend".into(),v::encode_blend_space(c.blend));}
        if let Some(resolution)=c.resolution {data.insert("resolution".into(),v::encode_resolution(resolution)?);}
        Ok(record(identity,"capy.composition/2",Value::Object(data)))
        },
        crate::Edit::Stack(change)=>{let identity=change.id;let stack=change.value.as_ref().ok_or("Cannot encode removed record")?;
            let entries=stack.entries.iter().map(|h|id(&art.occurrences,*h).map(reference)).collect::<Result<Vec<_>,_>>()?;
        Ok(record(identity,"capy.stack/1",if entries.is_empty(){json!({})}else{json!({"entries":entries})}))
        },
        crate::Edit::Paint(change)=>{let identity=change.id;let paint=change.value.as_ref().ok_or("Cannot encode removed record")?;
            if !paint.operations.is_empty() {return Err("Wait for the current edit before transferring".into());}
            let mut data=encode_raster(&paint.raster,paint.domain,false,canvas.color,paint.color_mode,resources,cancel)?;
        data.insert("color_mode".into(),serde_json::to_value(paint.color_mode).map_err(|e|e.to_string())?);
        if let Some(base)=&paint.base {
            base.validate(paint.domain,canvas.color).map_err(|e|e.to_string())?;
            let mut binding=json!({"image":encode_image(&base.image,resources)?});
            if base.offset!=[0;2] {binding["offset"]=json!(base.offset);}
            if base.policy==PaintBasePolicy::WorkingPixels {binding["policy"]="working_pixels".into();}
            data.insert("base".into(),binding);
        }
        Ok(record(identity,"capy.paint-source/2",Value::Object(data)))
        },
        crate::Edit::Coverage(change)=>{let identity=change.id;let coverage=change.value.as_ref().ok_or("Cannot encode removed record")?;
            if !coverage.operations.is_empty() {return Err("Wait for the current edit before transferring".into());}
            let mut data=encode_raster(&coverage.raster,coverage.domain,true,canvas.color,Default::default(),resources,cancel)?;
        if !(0. ..=1.).contains(&coverage.default_coverage) {return Err("Invalid default coverage".into());}
        set_float(&mut data,"default_coverage",coverage.default_coverage,1.)?;
        Ok(record(identity,"capy.coverage-source/2",Value::Object(data)))
        },
        crate::Edit::ObjectLayer(change)=>{let layer=change.value.as_ref().ok_or("Cannot encode removed record")?;
            let children=layer.children.iter().map(|h|id(&art.objects,*h).map(reference)).collect::<Result<Vec<_>,_>>()?;
            Ok(record(change.id,"capy.object-layer/1",json!({"children":children})))
        },
        crate::Edit::ImageObject(change)=>{let object=change.value.as_ref().ok_or("Cannot encode removed record")?;
            object.validate().map_err(|e|e.to_string())?;
            let mut data=json!({"image":encode_image(&object.image,resources)?}).as_object().unwrap().clone();
            set_name(&mut data,&object.name);set_bool(&mut data,"visible",object.visible,true);
            if object.affine!=Affine64::default() {data.insert("affine".into(),json!(object.affine.0));}
            if object.interpolation==ImageInterpolation::Nearest {data.insert("interpolation".into(),"nearest".into());}
            Ok(record(change.id,"capy.image-object/1",Value::Object(data)))
        },
        crate::Edit::Effect(change)=>{let effect=change.value.as_ref().ok_or("Cannot encode removed record")?;
            effect.validate().map_err(str::to_string)?;
            let program=effect_records::encode_program(&effect.program,resources)?;
            let mut data=if program.get("builtin").is_some() {program.as_object().unwrap().clone()}else {
                json!({"program":program}).as_object().unwrap().clone()
            };
            data.insert("values".into(),effect_records::encode_values(&effect.program,&effect.values,resources)?);
            if let Some(spatial)=&effect.spatial {data.insert("spatial".into(),json!({"mapping":spatial.mapping.0,"extent":spatial.extent}));}
            let record=record(change.id,"capy.effect/2",Value::Object(data));
            let context=if resources.private {super::registry::RecordContext::Private}else{super::registry::RecordContext::Portable};
            super::registry::validate_context(&record,context).map_err(|error|error.to_string())?;
            Ok(record)
        },
        crate::Edit::SavedSelection(change)=>{let identity=change.id;let selection=change.value.as_ref().ok_or("Cannot encode removed record")?;
        Ok(record(identity,"capy.selection/1",selection_records::encode_selection(&selection.selection,resources)?))
        },
        crate::Edit::Guides(change)=>{let identity=change.id;let guides=change.value.as_ref().ok_or("Cannot encode removed record")?;
            Ok(record(identity,"capy.guides/1",encode_guides(guides)?))
        },
        crate::Edit::Occurrence(change)=>{let identity=change.id;let occurrence=change.value.as_ref().ok_or("Cannot encode removed record")?;
            if !(0. ..=1.).contains(&occurrence.opacity) {return Err("Invalid occurrence opacity".into());}
        if occurrence.blend==LayerBlend::PassThrough && !matches!(occurrence.content,OccurrenceContent::Stack(_)) {return Err("Pass through requires a stack".into());}
        let content=match &occurrence.content {OccurrenceContent::Paint(h)=>json!({"paint":reference(id(&art.paint,*h)?)}),OccurrenceContent::Stack(h)=>json!({"stack":reference(id(&art.stacks,*h)?)}),OccurrenceContent::Effect(h)=>json!({"effect":reference(id(&art.effects,*h)?)}),OccurrenceContent::Selection(h)=>json!({"selection":reference(id(&art.selections,*h)?)}),OccurrenceContent::Objects(h)=>json!({"objects":reference(id(&art.object_layers,*h)?)})};
        let mut data=json!({"content":content}).as_object().unwrap().clone(); set_name(&mut data,&occurrence.name);
        for (key,value,default) in [("visible",occurrence.visible,true),("locked",occurrence.locked,false),("alpha_locked",occurrence.alpha_locked,false),("reference",occurrence.reference,false)] {set_bool(&mut data,key,value,default);}
        match occurrence.attachment {Attachment::None=>{},Attachment::Clip=>{data.insert("attachment".into(),json!("clip"));},Attachment::Effect=>{data.insert("attachment".into(),json!("effect"));}}
        set_float(&mut data,"opacity",occurrence.opacity,1.)?;
        if occurrence.blend!=LayerBlend::Normal {data.insert("blend".into(),v::encode_layer_blend(occurrence.blend));}
        if !occurrence.positioned() && occurrence.offset!=[0;2] {return Err("Effects and selections have no layer offset".into());}
        if occurrence.alpha_locked && !matches!(occurrence.content,OccurrenceContent::Paint(_)) {return Err("Only paint layers lock transparency".into());}
        if occurrence.offset!=[0;2] {data.insert("offset".into(),encode_offset(occurrence.offset));}
        if let Some(mask)=&occurrence.mask {
            let mut m=json!({"source":reference(id(&art.coverage,mask.source)?)}).as_object().unwrap().clone();
            set_bool(&mut m,"enabled",mask.enabled,true); set_bool(&mut m,"linked",mask.linked,true); set_bool(&mut m,"inverted",mask.inverted,false);
            if mask.offset!=[0;2] {m.insert("offset".into(),encode_offset(mask.offset));}
            data.insert("mask".into(),Value::Object(m));
        }
        Ok(record(identity,"capy.occurrence/3",Value::Object(data)))
        },
        crate::Edit::Output(change)=>{let identity=change.id;let output=change.value.as_ref().ok_or("Cannot encode removed record")?;
            let mut data=json!({"source":endpoint(id(&art.compositions,output.composition)?)}).as_object().unwrap().clone(); set_name(&mut data,&output.name);
        let mut context=Map::new();
        let mut seen=BTreeSet::new(); let mut phases=Vec::new();
        for (effect,phase) in output.context.phases.iter() {
            let effect=id(&art.effects,*effect)?;
            if !seen.insert(effect) || !phase.is_finite() {return Err("Invalid output effect phase".into());}
            if phase.to_bits()!=0 {phases.push(json!({"effect":reference(effect),"phase":f64::from(*phase)}));}
        }
        if !phases.is_empty() {context.insert("effect_phases".into(),phases.into());}
        optional_object(&mut data,"context",Value::Object(context));
        optional_object(&mut data,"sdr",v::encode_sdr(output.sdr)?);
        if let Some(proof)=&output.proof {data.insert("proof".into(),v::encode_proof(proof,|profile|resources.profile(profile))?);}
        Ok(record(identity,"capy.output/2",Value::Object(data)))
        },
        crate::Edit::Working(_)|crate::Edit::Batch(_)|crate::Edit::SetRaster{..}=>Err("Expected an authored record change".into()),
    }
}

pub fn encode(art:&Artwork,cancel:&AtomicBool)->Result<(Vec<Value>,ResourceInventory),String> {
    encode_with_inventory(art,cancel,ResourceInventory::default())
}
pub(crate) fn encode_with_inventory(art:&Artwork,cancel:&AtomicBool,mut resources:ResourceInventory)->Result<(Vec<Value>,ResourceInventory),String> {
    art.topology()?;
    let mut records=Vec::new();
    for (handle,id,value) in art.compositions.iter() {records.push(encode_change(art,&crate::Edit::Composition(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.stacks.iter() {records.push(encode_change(art,&crate::Edit::Stack(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.paint.iter() {records.push(encode_change(art,&crate::Edit::Paint(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.coverage.iter() {records.push(encode_change(art,&crate::Edit::Coverage(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.object_layers.iter() {records.push(encode_change(art,&crate::Edit::ObjectLayer(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.objects.iter() {records.push(encode_change(art,&crate::Edit::ImageObject(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.effects.iter() {records.push(encode_change(art,&crate::Edit::Effect(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.selections.iter() {records.push(encode_change(art,&crate::Edit::SavedSelection(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.guides.iter() {records.push(encode_change(art,&crate::Edit::Guides(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.occurrences.iter() {records.push(encode_change(art,&crate::Edit::Occurrence(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.outputs.iter() {records.push(encode_change(art,&crate::Edit::Output(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    art.metadata.validate()?;
    for (kind,block) in ["exif","xmp","iptc"].into_iter().zip(art.metadata.blocks()) {if let Some(bytes)=block {resources.bytes("capy.photo-metadata/1",bytes,json!({"kind":kind}))?;}}
    if cancel.load(Ordering::Relaxed) {return Err("Package operation cancelled".into());}
    records.extend(resources.images.values().cloned());
    Ok((records,resources))
}

fn payload(record:&Value)->DecodeResult<(&str,&Value)> {
    let data=fields(record,&["id","type","data","ancillary","copy_safe"])?;
    if bool_field(data,"ancillary",false)? || bool_field(data,"copy_safe",false)? {return Err("Known artwork cannot be ancillary".into());}
    Ok((v::string(v::required(data,"type")?)?,v::required(data,"data")?))
}
fn reserve<T>(store:&mut Store<T>,id:PortableId,strict:bool)->DecodeResult<()> {
    if store.allocated(id).is_none() {if strict {return Err("Object is absent from runtime handle layout".into());}store.reserve(id)?;}Ok(())
}
pub(crate) fn decode_records_into(art:&mut Artwork,objects:&BTreeMap<PortableId,Value>,reader:&mut ResourceReader<'_>)->DecodeResult<()> {
    for (identity,record) in objects {
        match record["type"].as_str().ok_or("Missing object type")? {
            "capy.composition/2"=>{if let Some(handle)=art.compositions.allocated(*identity) {art.compositions.remove(handle);}},
            "capy.stack/1"=>{if let Some(handle)=art.stacks.allocated(*identity) {art.stacks.remove(handle);}},
            "capy.occurrence/3"=>{if let Some(handle)=art.occurrences.allocated(*identity) {art.occurrences.remove(handle);}},
            "capy.paint-source/2"=>{if let Some(handle)=art.paint.allocated(*identity) {art.paint.remove(handle);}},
            "capy.coverage-source/2"=>{if let Some(handle)=art.coverage.allocated(*identity) {art.coverage.remove(handle);}},
            "capy.effect/2"=>{if let Some(handle)=art.effects.allocated(*identity) {art.effects.remove(handle);}},
            "capy.object-layer/1"=>{if let Some(handle)=art.object_layers.allocated(*identity) {art.object_layers.remove(handle);}},
            "capy.image-object/1"=>{if let Some(handle)=art.objects.allocated(*identity) {art.objects.remove(handle);}},
            "capy.selection/1"=>{if let Some(handle)=art.selections.allocated(*identity) {art.selections.remove(handle);}},
            "capy.guides/1"=>{if let Some(handle)=art.guides.allocated(*identity) {art.guides.remove(handle);}},
            "capy.output/2"=>{if let Some(handle)=art.outputs.allocated(*identity) {art.outputs.remove(handle);}},
            _=>{},
        }
    }
    let mut known=Vec::new();
    for (identity,record) in objects {
        let kind=record["type"].as_str().ok_or("Missing object type")?;
        if !super::manifest::known_object(kind) {
            if record["ancillary"].as_bool()==Some(true) {continue;}
            return Err(DecodeError::Unsupported(format!("Unknown authored object {kind}")));
        }
        let context=if reader.private {super::registry::RecordContext::Private}else{super::registry::RecordContext::Portable};
        super::registry::validate_context(record,context)?;
        let (kind,data)=payload(record)?;record_fields(kind,data)?;known.push((*identity,kind,data));
        match kind {"capy.composition/2"=>{reserve(&mut art.compositions,*identity,true)?;},"capy.stack/1"=>{reserve(&mut art.stacks,*identity,true)?;},"capy.occurrence/3"=>{reserve(&mut art.occurrences,*identity,true)?;},"capy.paint-source/2"=>{reserve(&mut art.paint,*identity,true)?;},"capy.coverage-source/2"=>{reserve(&mut art.coverage,*identity,true)?;},"capy.effect/2"=>{reserve(&mut art.effects,*identity,true)?;},"capy.object-layer/1"=>{reserve(&mut art.object_layers,*identity,true)?;},"capy.image-object/1"=>{reserve(&mut art.objects,*identity,true)?;},"capy.image/1"=>{},"capy.selection/1"=>{reserve(&mut art.selections,*identity,true)?;},"capy.guides/1"=>{reserve(&mut art.guides,*identity,true)?;},"capy.output/2"=>{reserve(&mut art.outputs,*identity,true)?;},_=>unreachable!()}

    }
    for (identity,kind,value) in &known {if *kind=="capy.composition/2" {
        let data=record_fields(kind,value)?;
        let size=v::parse_size(v::required(data,"size")?)?;
        if size.iter().any(|n|*n>reader.limits.dimension || *n>crate::MAX_EXTENT) {return Err(DecodeError::Unsupported("Composition exceeds dimension admission".into()));}
        let color=data.get("color").map(v::parse_document_color).transpose()?.unwrap_or(DocumentColor {space:crate::color::RgbSpace::Srgb,depth:crate::color::SampleDepth::U8});
        let blend=data.get("blend").map(v::parse_blend_space).transpose()?.unwrap_or(BlendSpace::Linear);
        if color.depth.is_float() && blend!=BlendSpace::Linear {return Err(DecodeError::Unsupported("Float compositions require linear blending".into()));}
        let c=Composition {size,color,blend,resolution:data.get("resolution").map(v::parse_resolution).transpose()?,result:endpoint_handle(&art.stacks,v::required(data,"result")?)?};
        art.compositions.install(art.compositions.allocated(*identity).unwrap(),c)?;
    }}
    let canvas=art.compositions.get(art.root).ok_or("Missing root composition")?.clone();
    for (identity,kind,value) in &known {match *kind {
        "capy.paint-source/2"=> {let data=record_fields(kind,value)?;let domain=dimension(v::required(data,"domain")?,reader)?;
            let color_mode=match data.get("color_mode").map(v::string).transpose()?.unwrap_or("full_color") {
                "full_color"=>crate::color::LayerColorMode::FullColor,"grayscale"=>crate::color::LayerColorMode::Grayscale,"two_tone"=>crate::color::LayerColorMode::TwoTone,
                name=>return Err(DecodeError::Unsupported(format!("Unknown paint color mode {name}"))),
            };
            let raster=decode_raster(data,domain,false,canvas.color,color_mode,reader)?;
            let base=data.get("base").map(|value| {
                let data=fields(value,&["image","offset","policy"])?;
                let policy=match data.get("policy").map(v::string).transpose()?.unwrap_or("source_profile") {
                    "source_profile"=>PaintBasePolicy::SourceProfile,"working_pixels"=>PaintBasePolicy::WorkingPixels,
                    name=>return Err(DecodeError::Unsupported(format!("Unknown paint base policy {name}"))),
                };
                let base=PaintBase {image:image_reference(v::required(data,"image")?,reader)?,offset:data.get("offset").map(coordinate).transpose()?.unwrap_or([0;2]),policy};
                base.validate(domain,canvas.color)?;Ok::<_,DecodeError>(base)
            }).transpose()?;
            art.paint.install(art.paint.allocated(*identity).unwrap(),PaintSource {color_mode,domain,raster,base,operations:Arc::default()})?;},
        "capy.image/1"=>{decode_image(*identity,value,reader)?;},
        "capy.image-object/1"=>{let data=record_fields(kind,value)?;
            let affine=data.get("affine").map(decode_affine).transpose()?.unwrap_or_default();
            let interpolation=match data.get("interpolation").map(v::string).transpose()?.unwrap_or("linear") {
                "linear"=>ImageInterpolation::Linear,"nearest"=>ImageInterpolation::Nearest,
                name=>return Err(DecodeError::Unsupported(format!("Unknown image interpolation {name}"))),
            };
            let object=ImageObject {image:image_reference(v::required(data,"image")?,reader)?,name:name(data)?,visible:bool_field(data,"visible",true)?,affine,interpolation};
            object.admit_affine().map_err(affine_error)?;object.validate()?;art.objects.install(art.objects.allocated(*identity).unwrap(),object)?;},
        "capy.object-layer/1"=>{let data=record_fields(kind,value)?;
            let children=list(v::required(data,"children")?)?.iter().map(|value|handle(&art.objects,value)).collect::<DecodeResult<Vec<_>>>()?;
            art.object_layers.install(art.object_layers.allocated(*identity).unwrap(),ObjectLayer {children})?;},
        "capy.coverage-source/2"=> {let data=record_fields(kind,value)?;let domain=dimension(v::required(data,"domain")?,reader)?;
            let raster=decode_raster(data,domain,true,canvas.color,Default::default(),reader)?;
            let default_coverage=float_field(data,"default_coverage",1.)?;if !(0. ..=1.).contains(&default_coverage) {return Err("Invalid default coverage".into());}
            art.coverage.install(art.coverage.allocated(*identity).unwrap(),CoverageSource {domain,raster,default_coverage,operations:Arc::default()})?;},
        "capy.selection/1"=> {let selection=selection_records::decode_selection(value,reader)?;
            art.selections.install(art.selections.allocated(*identity).unwrap(),SavedSelection {selection})?;},
        "capy.guides/1"=> {let guides=decode_guides(value)?;art.guides.install(art.guides.allocated(*identity).unwrap(),guides)?;}, _=>{}
    }}
    for (identity,kind,value) in &known {if *kind=="capy.effect/2" {
        let data=record_fields(kind,value)?;
        let program=if let Some(program)=data.get("program") {
            if data.contains_key("builtin") || data.contains_key("version") {return Err("Effect program alternatives are exclusive".into());}
            super::registry::validate_program_context(if reader.private {super::registry::RecordContext::Private}else{super::registry::RecordContext::Portable})?;
            let key=serde_json::to_vec(program).map_err(|e|DecodeError::Invalid(e.to_string()))?;
            if let Some(program)=reader.programs.get(&key) {program.clone()} else {let program=effect_records::decode_program(program,reader)?;reader.programs.insert(key,program.clone());program}
        } else {effect_records::decode_program(&json!({"builtin":v::required(data,"builtin")?,"version":v::required(data,"version")?}),reader)?};
        let values=effect_records::decode_values(&program,v::required(data,"values")?,reader)?;
        let spatial=data.get("spatial").map(|value| {
            let data=fields(value,&["mapping","extent"])?;let extent=v::array(v::required(data,"extent")?,2)?;
            let spatial=EffectSpatialReference {mapping:decode_affine(v::required(data,"mapping")?)?,extent:[finite_double(&extent[0])?,finite_double(&extent[1])?]};
            if spatial.extent.iter().any(|value|*value>f64::from(crate::MAX_EXTENT)) {return Err(DecodeError::Unsupported("Effect reference extent exceeds admission".into()));}
            spatial.validate()?;Ok::<_,DecodeError>(spatial)
        }).transpose()?;
        let effect=EffectApplication {program,values,spatial};effect.validate()?;
        art.effects.install(art.effects.allocated(*identity).unwrap(),effect)?;
    }}
    for (identity,kind,value) in &known {match *kind {
        "capy.stack/1"=> {let data=record_fields(kind,value)?;let entries=data.get("entries").map(|v|list(v)?.iter().map(|value|handle(&art.occurrences,value)).collect::<DecodeResult<Vec<_>>>()).transpose()?.unwrap_or_default();art.stacks.install(art.stacks.allocated(*identity).unwrap(),Stack {entries})?;},
        "capy.occurrence/3"=> {
            let data=record_fields(kind,value)?;
            let content=fields(v::required(data,"content")?,&["paint","objects","stack","effect","selection"])?;
            if content.len()!=1 {return Err("Occurrence requires one content alternative".into());}
            let (kind,value)=content.iter().next().unwrap();let content=match kind.as_str() {"paint"=>OccurrenceContent::Paint(handle(&art.paint,value)?),"objects"=>OccurrenceContent::Objects(handle(&art.object_layers,value)?),"stack"=>OccurrenceContent::Stack(handle(&art.stacks,value)?),"effect"=>OccurrenceContent::Effect(handle(&art.effects,value)?),"selection"=>OccurrenceContent::Selection(handle(&art.selections,value)?),_=>unreachable!()};
            let offset=data.get("offset").map(decode_offset).transpose()?.unwrap_or_default();
            let blend=data.get("blend").map(v::parse_layer_blend).transpose()?.unwrap_or(LayerBlend::Normal);
            if blend==LayerBlend::PassThrough && !matches!(content,OccurrenceContent::Stack(_)) {return Err("Pass through requires a stack".into());}
            let opacity=float_field(data,"opacity",1.)?;if !(0. ..=1.).contains(&opacity) {return Err("Invalid occurrence opacity".into());}
            let mask=data.get("mask").map(|value| {let data=fields(value,&["source","enabled","linked","inverted","offset"])?;
                Ok::<_,DecodeError>(MaskUse {source:handle(&art.coverage,v::required(data,"source")?)?,enabled:bool_field(data,"enabled",true)?,linked:bool_field(data,"linked",true)?,
                    inverted:bool_field(data,"inverted",false)?,offset:data.get("offset").map(decode_offset).transpose()?.unwrap_or_default()})}).transpose()?;
            let attachment=match data.get("attachment").map(v::string).transpose()? {None|Some("none")=>Attachment::None,Some("clip")=>Attachment::Clip,Some("effect")=>Attachment::Effect,Some(name)=>return Err(DecodeError::Unsupported(format!("Unknown occurrence attachment {name}")))};
            let occurrence=Occurrence {content,name:name(data)?,visible:bool_field(data,"visible",true)?,opacity,blend,locked:bool_field(data,"locked",false)?,alpha_locked:bool_field(data,"alpha_locked",false)?,reference:bool_field(data,"reference",false)?,attachment,offset,mask};
            if !occurrence.positioned() && data.contains_key("offset") || occurrence.alpha_locked && !matches!(occurrence.content,OccurrenceContent::Paint(_)) {return Err("Inapplicable occurrence fields".into());}
            art.occurrences.install(art.occurrences.allocated(*identity).unwrap(),occurrence)?;
        },_=>{}
    }}
    for (identity,kind,value) in &known {if *kind=="capy.output/2" {
        let data=record_fields(kind,value)?;
        let composition=endpoint_handle(&art.compositions,v::required(data,"source")?)?;
        let mut context=EvaluationContext::default();
        if let Some(value)=data.get("context") {let data=fields(value,&["effect_phases"])?;
            if let Some(value)=data.get("effect_phases") {let mut seen=BTreeSet::new();for phase in list(value)? {
                let data=fields(phase,&["effect","phase"])?;let effect=handle(&art.effects,v::required(data,"effect")?)?;
                let identity=art.effects.id(effect).unwrap();if !seen.insert(identity) {return Err("Duplicate output phase".into());}
                Arc::make_mut(&mut context.phases).push((effect,v::finite_f32(v::required(data,"phase")?)?));
            }}
        }
        let output=Output {composition,name:name(data)?,context,sdr:data.get("sdr").map(v::parse_sdr).transpose()?.unwrap_or(SdrRendition {exposure:0.,contrast:1.,headroom:2.3004484,highlight_color:0.3,balance:0.}),proof:data.get("proof").map(|v|v::parse_proof(v,|profile|reader.profile(profile))).transpose()?};
        art.outputs.install(art.outputs.allocated(*identity).unwrap(),output)?;
    }}
    Ok(())
}

pub fn decode(manifest:&Manifest,reader:&mut ResourceReader<'_>)->DecodeResult<Artwork> {
    decode_with_layout(manifest,reader,None)
}
pub(crate) fn decode_with_layout(manifest:&Manifest,reader:&mut ResourceReader<'_>,layout:Option<&super::transfer::TransferLayout>)->DecodeResult<Artwork> {
    let mut art=Artwork::new([1,1])?;
    art.id=manifest.document; art.compositions=Store::default();art.stacks=Store::default();art.occurrences=Store::default();art.paint=Store::default();art.coverage=Store::default();art.effects=Store::default();art.object_layers=Store::default();art.objects=Store::default();art.selections=Store::default();art.guides=Store::default();art.outputs=Store::default();
    if let Some(layout)=layout {layout.install(&mut art)?;}
    for (identity,record) in &manifest.objects {
        match record["type"].as_str().ok_or("Missing object type")? {
            "capy.composition/2"=>{reserve(&mut art.compositions,*identity,layout.is_some())?;},
            "capy.stack/1"=>{reserve(&mut art.stacks,*identity,layout.is_some())?;},
            "capy.paint-source/2"=>{reserve(&mut art.paint,*identity,layout.is_some())?;},
            "capy.coverage-source/2"=>{reserve(&mut art.coverage,*identity,layout.is_some())?;},
            "capy.object-layer/1"=>{reserve(&mut art.object_layers,*identity,layout.is_some())?;},
            "capy.image-object/1"=>{reserve(&mut art.objects,*identity,layout.is_some())?;},
            "capy.image/1"=>{},
            "capy.effect/2"=>{reserve(&mut art.effects,*identity,layout.is_some())?;},
            "capy.selection/1"=>{reserve(&mut art.selections,*identity,layout.is_some())?;},
            "capy.guides/1"=>{reserve(&mut art.guides,*identity,layout.is_some())?;},
            "capy.occurrence/3"=>{reserve(&mut art.occurrences,*identity,layout.is_some())?;},
            "capy.output/2"=>{reserve(&mut art.outputs,*identity,layout.is_some())?;},
            kind if record["ancillary"].as_bool()!=Some(true)=>return Err(DecodeError::Unsupported(format!("Unknown authored object {kind}"))),
            _=>{},
        }
    }
    art.root=art.compositions.allocated(manifest.root).ok_or("Root is not a composition")?;
    decode_records_into(&mut art,&manifest.objects,reader)?;
    if art.guides.iter().map(|(_,_,guides)|guides.rulers.len()).sum::<usize>()>crate::rulers::MAX_RULERS {
        return Err(DecodeError::Unsupported("Too many rulers".into()));
    }
    let mut metadata=PhotoMetadata::default();
    if let Some(value)=&manifest.metadata {
        let data=fields(value,&["exif","xmp","iptc"])?;
        for (kind,slot) in [("exif",&mut metadata.exif),("xmp",&mut metadata.xmp),("iptc",&mut metadata.iptc)] {if let Some(value)=data.get(kind) {
            let (_,record)=reader.record(value,"capy.photo-metadata/1")?;
            let descriptor=fields(&record["data"],&["kind","decoded_bytes"])?;
            if v::string(v::required(descriptor,"kind")?)?!=kind {return Err("Photo metadata kind mismatch".into());}
            *slot=Some(reader.bytes(value,"capy.photo-metadata/1",PhotoMetadata::MAX_BYTES)?);
        }}
    }
    if metadata.blocks().into_iter().flatten().map(|block|block.len()).sum::<usize>()>PhotoMetadata::MAX_BYTES {
        return Err(DecodeError::Unsupported("Photo metadata exceeds admission".into()));
    }
    metadata.validate()?;art.metadata=Arc::new(metadata);
    let default=manifest.default_output.ok_or_else(||DecodeError::Unsupported("Artwork has no editable output".into()))?;
    art.default_output=art.outputs.allocated(default).ok_or("Default is not an output")?;
    if matches!(manifest.support,Support::Editable) {
        let index=Arc::new(SceneIndex::build(&art)?);let scene=SceneView::new(&art,&index);
        if scene.order().iter().any(|h|!crate::offsets::admitted(scene.layer_origin(Some(*h))) || scene.mask_origin(*h).is_some_and(|origin|!crate::offsets::admitted(origin)))
            || art.occurrences.iter().any(|(_,_,o)|!crate::offsets::admitted(o.offset) || o.mask.as_ref().is_some_and(|m|!crate::offsets::admitted(m.offset))) {
            return Err(DecodeError::Unsupported("Layer offsets exceed the editor's range".into()));
        }
    }
    Ok(art)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point, raster::RasterTile};
    use super::super::{ImmutableBacking, manifest::{ResourceRecord,ResourceRange}};
    use std::io::Read;

    fn captured(art:&Artwork)->(Manifest,ImmutableBacking) {
        let cancel=AtomicBool::new(false);
        let (objects,inventory)=encode(art,&cancel).unwrap(); let prepared=inventory.prepare(&cancel).unwrap();
        let mut bytes=Vec::new();prepared.reader(&cancel).read_to_end(&mut bytes).unwrap();
        let resources=prepared.entries.iter().map(|entry| {
            let offset=super::super::manifest::decimal_u64(&entry.record["location"]["offset"]).unwrap();
            (entry.payload.id(),ResourceRecord {value:entry.record.clone(),bytes:entry.bytes,crc32:entry.crc,range:Some(ResourceRange {member:0,offset,length:entry.bytes})})
        }).collect();
        let mut metadata=Map::new();for (kind,block) in ["exif","xmp","iptc"].into_iter().zip(art.metadata.blocks()) {if let Some(block)=block {metadata.insert(kind.into(),reference(block.id()));}}
        let manifest=Manifest {document:art.id,root:art.compositions.id(art.root).unwrap(),objects:objects.into_iter().map(|record|(record["id"].as_str().unwrap().parse().unwrap(),record)).collect(),resources,
            outputs:art.outputs.iter().map(|(_,id,_)|id).collect(),default_output:Some(art.outputs.id(art.default_output).unwrap()),metadata:(!metadata.is_empty()).then_some(Value::Object(metadata)),support:Support::Editable};
        let bytes:Arc<[u8]>=bytes.into(); (manifest,ImmutableBacking::new(Arc::new(bytes)).unwrap())
    }
    fn reopen(manifest:&Manifest,backing:&ImmutableBacking)->DecodeResult<Artwork> {let cancel=AtomicBool::new(false); decode(manifest,&mut ResourceReader::new(manifest,backing,&cancel,crate::ProjectLimits::default()))}
    fn object_data(art:&Artwork)->BTreeMap<PortableId,Value> {encode(art,&AtomicBool::new(false)).unwrap().0.into_iter().map(|record|(record["id"].as_str().unwrap().parse().unwrap(),record)).collect()}
    fn fixture()->Artwork {
        let mut art=Artwork::new([37,29]).unwrap();let color=art.compositions.get(art.root).unwrap().color;
        let mut raster=RasterData::default();raster.watercolor=Some(RasterWatercolor {wet_edge:0.25,burnt_edge:0.3,edge_width:3.});
        for plane in [RasterPlane::Color,RasterPlane::WatercolorWetness] {
            let descriptor=plane.descriptor(color);let bytes=vec![73;descriptor.byte_len([TILE_SIZE;2]).unwrap()];
            raster.tiles.insert(TileKey {plane,coordinate:[0,0]},RasterTile::backed(crate::raster::TileBlob::encode(descriptor,&bytes).unwrap()));
        }
        let original=crate::color::source::rgba8_source([37,29],|x,y|[x as u8,y as u8,71,0]);
        let paint=art.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:[37,29],raster:RasterRevision::backed(raster),base:Some(PaintBase::new(Image::new(original))),operations:Arc::default()}).unwrap();
        let coverage=art.coverage.insert(PortableId::random(),CoverageSource {domain:[37,29],raster:RasterRevision::default(),default_coverage:0.25,operations:Arc::default()}).unwrap();
        let mut occurrence=Occurrence::new(OccurrenceContent::Paint(paint),"Paint");occurrence.visible=false;occurrence.opacity=0.625;occurrence.offset=[-1300,7];occurrence.alpha_locked=true;occurrence.blend=LayerBlend::Multiply;
        occurrence.mask=Some(MaskUse {source:coverage,enabled:false,linked:false,inverted:true,offset:[1,2]});
        let occurrence=art.occurrences.insert(PortableId::random(),occurrence).unwrap();
        let inner=art.stacks.insert(PortableId::random(),Stack {entries:vec![occurrence]}).unwrap();
        let mut group=Occurrence::new(OccurrenceContent::Stack(inner),"Group");group.blend=LayerBlend::PassThrough;
        let group=art.occurrences.insert(PortableId::random(),group).unwrap();
        let stack=art.compositions.get(art.root).unwrap().result;art.stacks.get_mut(stack).unwrap().entries.push(group);
        art.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:[3,5],raster:RasterRevision::default(),base:None,operations:Arc::default()}).unwrap();
        art.selections.insert(PortableId::random(),SavedSelection {selection:crate::Selection::full(),}).unwrap();
        art.selections.insert(PortableId::random(),SavedSelection {selection:crate::Selection::empty(),}).unwrap();
        art.guides.insert(PortableId::random(),Guides {rulers:vec![(PortableId::random(),RulerGeometry::Parallel {start:Point {x:1.,y:2.},end:Point {x:3.,y:4.}})]}).unwrap();
        let program=crate::bundled_effect_catalog().filters()[0].program();let values=crate::EffectInstance::new(program.clone()).values;
        let effect=art.effects.insert(PortableId::random(),EffectApplication::new(program,values,[37,29])).unwrap();
        let output=art.outputs.get_mut(art.default_output).unwrap();output.context.phases=vec![(effect,0.75)].into();output.name="Export".into();output.sdr.exposure=0.625;
        output.proof=Some(crate::color::ProofRecipe::new("Print".into(),crate::color::ColorProfile::Builtin(crate::color::RgbSpace::AdobeRgb)));
        art.metadata=Arc::new(PhotoMetadata {exif:Some(Resource::from(vec![7;61])),xmp:Some(Resource::from(vec![11;71])),iptc:None});art
    }
    #[test]
    fn fixed_shared_image_fixture_preserves_authored_bits_and_bindings_across_all_transports() {
        use crate::package::{codec::OpenOutcome,session::PreparedSession,session_transfer::PreparedSessionTransfer};
        let bytes:Arc<[u8]>=Arc::from(include_bytes!("codec/fixtures/shared-image-objects.capy").as_slice());
        let backing=ImmutableBacking::new(Arc::new(bytes)).unwrap();let cancel=AtomicBool::new(false);let limits=crate::ProjectLimits::default();
        let OpenOutcome::Candidate {artwork,..}=crate::package::codec::open(backing,limits,&cancel).unwrap() else {panic!("Fixed object artwork must be editable")};
        let identity=|value:u128|PortableId::from_bytes(value.to_be_bytes());
        let expected=[
            [0x3ff0000000000000,0x3fc0000000000000,0x3fd0000000000000,0x3fec000000000000,0x4170000012000000,0xc160000028000000],
            [0xbff4000000000000,0x3fe0000000000000,0x3fc0000000000000,0x4000000000000000,0xc0934a4584fd0fdf,0x40c34a4587f00967],
        ];
        let verify=|art:&Artwork| {
            let source=art.paint.get(art.paint.resolve(identity(7)).unwrap()).unwrap().base.as_ref().unwrap();
            assert_eq!(source.image.id(),identity(11));assert_eq!(source.offset,[3,5]);assert_eq!(source.policy,PaintBasePolicy::SourceProfile);
            for (index,bits) in expected.iter().enumerate() {let object=art.objects.get(art.objects.resolve(identity(9+index as u128)).unwrap()).unwrap();
                assert_eq!(object.affine.0.map(f64::to_bits),*bits);assert!(object.image.same_owner(&source.image));
            }
            let working=art.paint.get(art.paint.resolve(identity(8)).unwrap()).unwrap().base.as_ref().unwrap();
            assert_eq!(working.offset,[7,11]);assert_eq!(working.policy,PaintBasePolicy::WorkingPixels);assert_eq!(working.image.id(),identity(12));
            let mut row=vec![0;source.image.row_bytes()];source.image.rows().read(1,&mut row).unwrap();
            assert_eq!(u16::from_le_bytes(row[0..2].try_into().unwrap()),256);assert_eq!(u16::from_le_bytes(row[510..512].try_into().unwrap()),511);
            let mut row=vec![0;working.image.row_bytes()];working.image.rows().read(0,&mut row).unwrap();
            let bits=row[..32].chunks_exact(4).map(|bytes|u32::from_le_bytes(bytes.try_into().unwrap())).collect::<Vec<_>>();
            assert_eq!(bits,[1,0x80000000,0xbe800000,0,0x7f7fffff,0x40080000,0x3f000000,0x3f800000]);
        };
        verify(&artwork);let (manifest,backing)=captured(&artwork);verify(&reopen(&manifest,&backing).unwrap());
        let mut editor=crate::Editor::new(crate::Document::from_artwork(artwork).unwrap());
        let object=editor.document().artwork.objects.resolve(identity(9)).unwrap();
        let mut changed=editor.document().artwork.objects.get(object).unwrap().clone();changed.affine.0=expected[1].map(f64::from_bits);
        editor.perform(crate::Edit::ImageObject(crate::RecordChange::replace(&editor.document().artwork.objects,object,Some(changed)).unwrap())).unwrap();editor.undo().unwrap();
        let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        let session=PreparedSession::prepare(&capture,json!({}),&cancel).unwrap();let mut pack=Vec::new();session.resources().reader(&cancel).read_to_end(&mut pack).unwrap();
        let mut loaded=crate::package::session::open_parts(session.metadata(),ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack))).unwrap(),limits,&cancel).unwrap();verify(&loaded.editor.document().artwork);
        loaded.editor.redo().unwrap();assert_eq!(loaded.editor.document().artwork.objects.get(object).unwrap().affine.0.map(f64::to_bits),expected[1]);
        loaded.editor.undo().unwrap();verify(&loaded.editor.document().artwork);
        let transfer=PreparedSessionTransfer::capture(&capture,json!({}),&cancel).unwrap();
        let descriptor=serde_json::from_slice(&serde_json::to_vec(transfer.descriptor()).unwrap()).unwrap();
        let mut receiver=crate::package::session_transfer::SessionTransferReceiver::new(descriptor,limits).unwrap();
        for index in receiver.missing_payloads() {let mut offset=0;let length=transfer.payload_len(index).unwrap();while offset<length {
            let count=(length-offset).min(crate::package::MAX_RANGE_BYTES as u64) as usize;
            receiver.push_chunk(index,&transfer.read_chunk(index,offset,count).unwrap()).unwrap();offset+=count as u64;
        }}
        let mut loaded=receiver.finish().unwrap().adopt_verified(limits,&cancel).unwrap();verify(&loaded.editor.document().artwork);
        loaded.editor.redo().unwrap();assert_eq!(loaded.editor.document().artwork.objects.get(object).unwrap().affine.0.map(f64::to_bits),expected[1]);
        loaded.editor.undo().unwrap();verify(&loaded.editor.document().artwork);
    }
    #[test]
    fn shared_images_objects_and_paint_bindings_preserve_identity_and_f64_geometry() {
        let mut art=Artwork::new([37,29]).unwrap();
        let image=Image::new(crate::color::source::rgba8_source([11,7],|x,y|[x as u8,y as u8,71,255]));
        let mut children=Vec::new();
        for affine in [Affine64::default(),Affine64([0.,-1.,1.,0.,16_777_217.125,-8_388_609.25])] {
            let mut object=ImageObject::new(image.clone(),"Photo");object.affine=affine;
            children.push(art.objects.insert(PortableId::random(),object).unwrap());
        }
        let layer=art.object_layers.insert(PortableId::random(),ObjectLayer {children:children.clone()}).unwrap();
        let occurrence=art.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(layer),"Photos")).unwrap();
        let stack=art.compositions.get(art.root).unwrap().result;art.stacks.get_mut(stack).unwrap().entries.push(occurrence);
        let paint=art.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:[37,29],base:Some(PaintBase {image:image.clone(),offset:[3,5],policy:PaintBasePolicy::SourceProfile}),raster:Default::default(),operations:Arc::default()}).unwrap();
        let (manifest,backing)=captured(&art);assert_eq!(manifest.objects.values().filter(|r|r["type"]=="capy.image/1").count(),1);
        assert_eq!(manifest.objects[&art.occurrences.id(occurrence).unwrap()]["type"],"capy.occurrence/3");
        let restored=reopen(&manifest,&backing).unwrap();let restored_image=&restored.paint.get(restored.paint.resolve(art.paint.id(paint).unwrap()).unwrap()).unwrap().base.as_ref().unwrap().image;
        for child in children {let object=restored.objects.get(restored.objects.resolve(art.objects.id(child).unwrap()).unwrap()).unwrap();
            assert_eq!(object.image.id(),image.id());assert!(object.image.same_owner(restored_image));
            assert_eq!(object.affine.0.map(f64::to_bits),art.objects.get(child).unwrap().affine.0.map(f64::to_bits));
        }
    }
    #[test]
    fn occurrence_offsets_are_canonical_signed_decimal_strings_with_owner_relative_masks() {
        let mut art=fixture();let (handle,identity,_)=art.occurrences.iter().next().unwrap();
        let occurrence=art.occurrences.get_mut(handle).unwrap();occurrence.offset=[-16_777_216,3];
        let mask=occurrence.mask.as_mut().unwrap();mask.linked=true;mask.offset=[9,-2];
        let encoded=object_data(&art);assert_eq!(encoded[&identity]["type"],"capy.occurrence/3");
        assert_eq!(encoded[&identity]["data"]["offset"],json!(["-16777216","3"]));assert_eq!(encoded[&identity]["data"]["mask"]["offset"],json!(["9","-2"]));
        let (mut manifest,backing)=captured(&art);let restored=reopen(&manifest,&backing).unwrap();assert_eq!(object_data(&restored),encoded);
        let original=manifest.objects[&identity].clone();
        let mut reopened=|record:Value| {manifest.objects.insert(identity,record);let result=reopen(&manifest,&backing);manifest.objects.insert(identity,original.clone());result};
        let data=|key:&str,value:Value| {let mut record=original.clone();record["data"][key]=value;record};
        for noncanonical in [json!(["+1","0"]),json!(["-0","0"]),json!(["01","0"]),json!([1,0]),json!(["1"]),json!(["9223372036854775808","0"])] {
            assert!(matches!(reopened(data("offset",noncanonical.clone())),Err(DecodeError::Invalid(_))),"{noncanonical}");
        }
        assert!(matches!(reopened(data("offset",json!(["16777217","0"]))),Err(DecodeError::Unsupported(_))));
        assert!(matches!(reopened(data("offset",json!(["-9223372036854775808","0"]))),Err(DecodeError::Unsupported(_))));
        assert!(matches!(reopened(data("placement",json!({"translation":[1,2]}))),Err(DecodeError::Unsupported(_))));
        let mut legacy=original.clone();legacy["type"]="capy.occurrence/2".into();
        assert!(matches!(reopened(legacy),Err(DecodeError::Unsupported(_))));
        let group=art.occurrences.iter().find(|(_,_,o)|matches!(o.content,OccurrenceContent::Stack(_))).unwrap().1;
        let mut manifest=captured(&art).0;manifest.objects.get_mut(&group).unwrap()["data"]["alpha_locked"]=true.into();
        assert!(matches!(reopen(&manifest,&backing),Err(DecodeError::Invalid(_))));
    }
    #[test]
    fn conflicting_immutable_image_ids_fail_across_reader_states() {
        let image=Image::new(crate::color::source::rgba8_source([2,2],|_,_|[1,2,3,255]));
        let mut resources=ResourceInventory::default();let reference=encode_image(&image,&mut resources).unwrap();
        let art=Artwork::new([2,2]).unwrap();let (manifest,backing)=captured(&art);let cancel=AtomicBool::new(false);
        let mut reader=ResourceReader::new(&manifest,&backing,&cancel,crate::ProjectLimits::default());
        let data=resources.images[&image.id()]["data"].clone();
        let mut descriptor=data.clone();descriptor["interpretation"]["profile_assumed"]=true.into();
        reader.images.insert(image.id(),(data,image.clone()));
        assert!(matches!(decode_image(image.id(),&descriptor,&mut reader),Err(DecodeError::Invalid(_))));
        let replacement=Image::with_id(image.id(),crate::color::source::rgba8_source([3,2],|_,_|[1,2,3,255]));
        assert!(encode_image(&replacement,&mut resources).is_err());assert_eq!(reference,super::super::resources::reference(image.id()));
    }
    #[test]
    fn records_preserve_all_stores_unplaced_work_and_compressed_resource_bytes() {
        let art=fixture();let expected=object_data(&art);let (manifest,backing)=captured(&art);let restored=reopen(&manifest,&backing).unwrap();
        assert_eq!(object_data(&restored),expected);assert_eq!(restored.id,art.id);assert_eq!(restored.metadata,art.metadata);
        assert_eq!(restored.paint.iter().count(),2);assert_eq!(restored.occurrences.iter().count(),2);
        let (_,before)=encode(&art,&AtomicBool::new(false)).unwrap();let (_,after)=encode(&restored,&AtomicBool::new(false)).unwrap();
        assert_eq!(before.entries.len(),after.entries.len());
        for (id,entry) in before.entries {let restored=&after.entries[&id];assert_eq!(entry.kind,restored.kind);assert_eq!(entry.data,restored.data);assert_eq!(entry.payload.encoded().unwrap().bytes,restored.payload.encoded().unwrap().bytes);}
    }
    #[test]
    fn minimal_records_omit_frozen_defaults() {
        let art=Artwork::new([11,17]).unwrap();let (manifest,backing)=captured(&art);
        assert_eq!(manifest.resources.len(),0);assert!(manifest.metadata.is_none());
        assert_eq!(manifest.objects[&art.stacks.iter().next().unwrap().1]["data"],json!({}));
        assert_eq!(manifest.objects[&art.outputs.id(art.default_output).unwrap()]["data"],json!({"source":endpoint(art.compositions.id(art.root).unwrap())}));
        assert_eq!(object_data(&reopen(&manifest,&backing).unwrap()),object_data(&art));
    }
    #[test]
    fn every_retained_known_record_rejects_invalid_and_unknown_fields() {
        let art=fixture();let (mut manifest,backing)=captured(&art);let paint=art.paint.iter().last().unwrap().1;
        let original=manifest.objects[&paint].clone();manifest.objects.get_mut(&paint).unwrap()["data"]["domain"]["size"]=json!([0,5]);
        assert!(matches!(reopen(&manifest,&backing),Err(DecodeError::Invalid(_))));
        manifest.objects.insert(paint,original.clone());manifest.objects.get_mut(&paint).unwrap()["data"]["future"]=true.into();
        assert!(matches!(reopen(&manifest,&backing),Err(DecodeError::Unsupported(_))));
        manifest.objects.insert(paint,original);let occurrence=art.occurrences.iter().next().unwrap().1;let original=manifest.objects[&occurrence].clone();
        manifest.objects.get_mut(&occurrence).unwrap()["data"]["mask"]["placement"]["interpolation"]="nearest".into();
        assert!(matches!(reopen(&manifest,&backing),Err(DecodeError::Unsupported(_))));
        manifest.objects.insert(occurrence,original.clone());manifest.objects.get_mut(&occurrence).unwrap()["data"]["attachment"]="future".into();
        assert!(matches!(reopen(&manifest,&backing),Err(DecodeError::Unsupported(_))));
        for attachment in ["clip","effect"] {manifest.objects.insert(occurrence,original.clone());manifest.objects.get_mut(&occurrence).unwrap()["data"]["attachment"]=attachment.into();assert!(matches!(reopen(&manifest,&backing),Err(DecodeError::Invalid(_))));}
    }
    #[test]
    fn tile_coordinates_and_output_phase_duplicates_are_invalid() {
        let art=fixture();let (mut manifest,backing)=captured(&art);let paint=art.paint.iter().next().unwrap().1;
        let original=manifest.objects[&paint].clone();let tile=manifest.objects[&paint]["data"]["tiles"][0].clone();
        manifest.objects.get_mut(&paint).unwrap()["data"]["tiles"].as_array_mut().unwrap().push(tile);
        assert!(matches!(reopen(&manifest,&backing),Err(DecodeError::Invalid(_))));
        manifest.objects.insert(paint,original);let output=art.outputs.id(art.default_output).unwrap();
        let phase=manifest.objects[&output]["data"]["context"]["effect_phases"][0].clone();
        manifest.objects.get_mut(&output).unwrap()["data"]["context"]["effect_phases"].as_array_mut().unwrap().push(phase);
        assert!(matches!(reopen(&manifest,&backing),Err(DecodeError::Invalid(_))));
    }
    #[test]
    fn shared_known_content_still_validates_every_retained_payload() {
        let mut art=fixture();let paint=art.paint.iter().next().unwrap().0;
        art.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),"Shared unplaced paint")).unwrap();
        let shape=art.topology().unwrap();let root=art.compositions.id(art.root).unwrap();
        let support=shape.validate(root,GraphLimits::default()).unwrap();assert!(matches!(support,Support::Preserved(_)));
        let (mut manifest,backing)=captured(&art);manifest.support=support;
        assert!(reopen(&manifest,&backing).is_ok());
        let unplaced=art.paint.iter().last().unwrap().1;
        manifest.objects.get_mut(&unplaced).unwrap()["data"]["domain"]["size"]=json!([3,0]);
        assert!(matches!(reopen(&manifest,&backing),Err(DecodeError::Invalid(_))));
    }

}
