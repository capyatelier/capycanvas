use super::{effect_records, selection_records, manifest::Manifest, resources::{ResourceInventory, ResourceReader, reference, reference_id}, values::{self as v, DecodeError, DecodeResult}};
use super::RASTER_TILE_SIZE as TILE_SIZE;
use crate::{authored::*, color::{DocumentColor, hdr::SdrRendition, source::{SourceImage, SourceKind, SourceChannels, SourceInterpretation}},
    raster::{RasterData, RasterRevision, RasterPlane, RasterWatercolor, TileKey},
    BlendSpace, LayerBlend, LayerPlacement, Point, Rect, PhotoMetadata, RulerGeometry};
use serde_json::{Map, Value, json};
use std::{collections::{BTreeMap, BTreeSet}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

fn record(id:PortableId, kind:&str, data:Value)->Value {json!({"id":id,"type":kind,"data":data})}
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
fn encode_raster(raster:&RasterRevision, domain:[u32;2], mask:bool,color:DocumentColor,resources:&mut ResourceInventory,cancel:&AtomicBool)->Result<Map<String,Value>,String> {
    let raster=raster.wait_data_cancellable(cancel)?; raster.validate_index(domain,mask,color)?;
    let RasterData {tiles:raster_tiles,watercolor}=raster.as_ref();
    let mut data=Map::new();
    if !raster_tiles.is_empty() {
        let mut tiles=Vec::new();
        for (key,tile) in raster_tiles {
            if cancel.load(Ordering::Relaxed) {return Err("Package operation cancelled".into());}
            let backing=tile.wait_backing_cancellable(cancel)?;
            if backing.descriptor!=key.plane.descriptor(color) {return Err("Wrong raster tile interpretation".into());}
            tiles.push(json!({"coordinate":key.coordinate,"plane":plane_name(key.plane),"resource":resources.tile(backing)?}));
        }
        data.insert("tiles".into(),Value::Array(tiles));
    }
    if let Some(RasterWatercolor {wet_edge,burnt_edge,edge_width})=watercolor {
        data.insert("material".into(),json!({"watercolor":{"wet_edge":f64::from(*wet_edge),"burnt_edge":f64::from(*burnt_edge),"edge_width":f64::from(*edge_width)}}));
    }
    data.insert("domain".into(),v::encode_domain(domain)?); Ok(data)
}
fn decode_raster(data:&Map<String,Value>,domain:[u32;2],mask:bool,color:DocumentColor,reader:&mut ResourceReader<'_>)->DecodeResult<RasterRevision> {
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
    raster.validate(domain,mask,color)?; Ok(RasterRevision::backed(raster))
}
fn validate_original_color(source:&SourceImage,color:DocumentColor)->Result<(),String> {
    if !source.is_original() && (source.interpretation.depth!=color.depth || source.interpretation.profile!=crate::color::ColorProfile::Builtin(color.space)) {
        return Err("Rasterized image interpretation differs from the document".into());
    }
    Ok(())
}
fn encode_original(source:&SourceImage,resources:&mut ResourceInventory)->Result<Value,String> {
    source.validate()?;
    let channels=match source.interpretation.channels {SourceChannels::Gray=>"gray",SourceChannels::GrayAlpha=>"gray_alpha",SourceChannels::Rgb=>"rgb",SourceChannels::Rgba=>"rgba",SourceChannels::Cmyk=>"cmyk"};
    let mut interpretation=json!({"channels":channels,"depth":v::encode_depth(source.interpretation.depth),"profile":resources.profile(&source.interpretation.profile)?});
    if source.interpretation.profile_assumed {interpretation["profile_assumed"]=true.into();}
    let mut data=json!({"extent":v::encode_size(source.extent)?,"interpretation":interpretation,"tiles":source.tiles.iter().map(|(coordinate,tile)|Ok(json!({"coordinate":coordinate,"resource":resources.tile(tile.clone())?}))).collect::<Result<Vec<_>,String>>()?});
    match source.kind {SourceKind::Original=>{},SourceKind::Rasterized=>{data["role"]="rasterized".into();}}
    if let Some(resolution)=source.resolution {data["resolution"]=v::encode_resolution(resolution)?;}
    Ok(data)
}
fn decode_original(value:&Value,reader:&mut ResourceReader<'_>)->DecodeResult<Arc<SourceImage>> {
    let key=serde_json::to_string(value).map_err(|e|DecodeError::Invalid(e.to_string()))?;
    if let Some(source)=reader.originals.get(&key) {return Ok(source.clone());}
    let data=fields(value,&["extent","interpretation","tiles","role","resolution"])?;
    let extent=v::parse_size(v::required(data,"extent")?)?;
    if extent.iter().any(|n|*n>reader.limits.dimension) {return Err(DecodeError::Unsupported("Original exceeds dimension admission".into()));}
    let interpretation=fields(v::required(data,"interpretation")?,&["channels","depth","profile","profile_assumed"])?;
    let channels=match v::string(v::required(interpretation,"channels")?)? {"gray"=>SourceChannels::Gray,"gray_alpha"=>SourceChannels::GrayAlpha,"rgb"=>SourceChannels::Rgb,"rgba"=>SourceChannels::Rgba,"cmyk"=>SourceChannels::Cmyk,name=>return Err(DecodeError::Unsupported(format!("Unknown source channels {name}")))};
    let interpretation=SourceInterpretation {channels,depth:v::parse_depth(v::required(interpretation,"depth")?)?,profile:reader.profile(v::required(interpretation,"profile")?)?,profile_assumed:bool_field(interpretation,"profile_assumed",false)?};
    if interpretation.depth.is_float() && (!matches!(interpretation.profile,crate::color::ColorProfile::Builtin(_)) || !matches!(channels,SourceChannels::Rgb|SourceChannels::Rgba)) {
        return Err(DecodeError::Unsupported("HDR source interpretation is unsupported".into()));
    }
    let kind=match data.get("role").map(v::string).transpose()?.unwrap_or("original") {"original"=>SourceKind::Original,"rasterized"=>SourceKind::Rasterized,name=>return Err(DecodeError::Unsupported(format!("Unknown original role {name}")))};
    let mut tiles=BTreeMap::new(); let records=list(v::required(data,"tiles")?)?;
    if records.len()>reader.limits.tiles {return Err(DecodeError::Unsupported("Too many original tiles".into()));}
    for value in records {
        let data=fields(value,&["coordinate","resource"])?; let coordinate=coordinate(v::required(data,"coordinate")?)?;
        if tiles.contains_key(&coordinate) || coordinate[0]>=extent[0].div_ceil(TILE_SIZE) || coordinate[1]>=extent[1].div_ceil(TILE_SIZE) {return Err("Invalid original tile index".into());}
        let tile=reader.tile(v::required(data,"resource")?)?;
        tiles.insert(coordinate,tile);
    }
    let source=SourceImage {extent,interpretation,kind,tiles,resolution:data.get("resolution").map(v::parse_resolution).transpose()?}; source.validate()?; let source=Arc::new(source); reader.originals.insert(key,source.clone()); Ok(source)
}
fn content_bounds(art:&Artwork,content:&OccurrenceContent,canvas:[u32;2])->Result<Rect,String> {
    Ok(match content {OccurrenceContent::Paint(h)=>Rect::from_extent(art.paint.get(*h).ok_or("Missing paint source")?.domain),
        OccurrenceContent::Selection(h)=> {let bounds=art.selections.get(*h).ok_or("Missing selection")?.selection.bounds();if bounds.is_empty() {Rect::from_extent(canvas)} else {bounds}},
        OccurrenceContent::Stack(_)|OccurrenceContent::Effect(_)=>Rect::from_extent(canvas)})
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
        let mut data=json!({"frame":v::encode_frame(c.origin,c.size)?,"result":endpoint(id(&art.stacks,c.result)?)}).as_object().unwrap().clone();
        optional_object(&mut data,"color",v::encode_document_color(c.color)?);
        if c.blend!=BlendSpace::Linear {data.insert("blend".into(),v::encode_blend_space(c.blend));}
        if let Some(resolution)=c.resolution {data.insert("resolution".into(),v::encode_resolution(resolution)?);}
        Ok(record(identity,"capy.composition/1",Value::Object(data)))
        },
        crate::Edit::Stack(change)=>{let identity=change.id;let stack=change.value.as_ref().ok_or("Cannot encode removed record")?;
            let entries=stack.entries.iter().map(|h|id(&art.occurrences,*h).map(reference)).collect::<Result<Vec<_>,_>>()?;
        Ok(record(identity,"capy.stack/1",if entries.is_empty(){json!({})}else{json!({"entries":entries})}))
        },
        crate::Edit::Paint(change)=>{let identity=change.id;let paint=change.value.as_ref().ok_or("Cannot encode removed record")?;
            if !paint.operations.is_empty() {return Err("Wait for the current edit before transferring".into());}
            let mut data=encode_raster(&paint.raster,paint.domain,false,canvas.color,resources,cancel)?;
        if let Some(source)=&paint.original {validate_original_color(source,canvas.color)?;data.insert("original".into(),encode_original(source,resources)?);}
        Ok(record(identity,"capy.paint-source/1",Value::Object(data)))
        },
        crate::Edit::Coverage(change)=>{let identity=change.id;let coverage=change.value.as_ref().ok_or("Cannot encode removed record")?;
            if !coverage.operations.is_empty() {return Err("Wait for the current edit before transferring".into());}
            let mut data=encode_raster(&coverage.raster,coverage.domain,true,canvas.color,resources,cancel)?;
        if !(0. ..=1.).contains(&coverage.default_coverage) {return Err("Invalid default coverage".into());}
        set_float(&mut data,"default_coverage",coverage.default_coverage,1.)?;
        if let Some(selection)=&coverage.initial {data.insert("initial".into(),selection_records::encode_selection(selection,resources)?);}
        Ok(record(identity,"capy.coverage-source/1",Value::Object(data)))
        },
        crate::Edit::Definition(change)=>{let identity=change.id;let definition=change.value.as_ref().ok_or("Cannot encode removed record")?;
            Ok(record(identity,"capy.effect-definition/1",effect_records::encode_definition(definition,resources)?))
        },
        crate::Edit::Effect(change)=>{let identity=change.id;let effect=change.value.as_ref().ok_or("Cannot encode removed record")?;
            let definition=art.definitions.get(effect.definition).ok_or("Missing effect definition")?;
        let mut data=json!({"definition":reference(id(&art.definitions,effect.definition)?)}).as_object().unwrap().clone();
        optional_object(&mut data,"values",effect_records::encode_values(&definition.program,&effect.values,resources)?);
        Ok(record(identity,"capy.effect/1",Value::Object(data)))
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
        let content=match &occurrence.content {OccurrenceContent::Paint(h)=>json!({"paint":reference(id(&art.paint,*h)?)}),OccurrenceContent::Stack(h)=>json!({"stack":reference(id(&art.stacks,*h)?)}),OccurrenceContent::Effect(h)=>json!({"effect":reference(id(&art.effects,*h)?)}),OccurrenceContent::Selection(h)=>json!({"selection":reference(id(&art.selections,*h)?)})};
        let mut data=json!({"content":content}).as_object().unwrap().clone(); set_name(&mut data,&occurrence.name);
        for (key,value,default) in [("visible",occurrence.visible,true),("locked",occurrence.locked,false),("alpha_locked",occurrence.alpha_locked,false),("reference",occurrence.reference,false)] {set_bool(&mut data,key,value,default);}
        match occurrence.attachment {Attachment::None=>{},Attachment::Clip=>{data.insert("attachment".into(),json!("clip"));},Attachment::Effect=>{data.insert("attachment".into(),json!("effect"));}}
        set_float(&mut data,"opacity",occurrence.opacity,1.)?;
        if occurrence.blend!=LayerBlend::Normal {data.insert("blend".into(),v::encode_layer_blend(occurrence.blend));}
        optional_object(&mut data,"placement",v::encode_placement(occurrence.translation,&occurrence.placement,content_bounds(art,&occurrence.content,canvas.size)?)?);
        if let Some(mask)=&occurrence.mask {
            let source=art.coverage.get(mask.source).ok_or("Missing coverage source")?;
            let mut m=json!({"source":reference(id(&art.coverage,mask.source)?)}).as_object().unwrap().clone();
            set_bool(&mut m,"enabled",mask.enabled,true); set_bool(&mut m,"linked",mask.linked,true); set_bool(&mut m,"inverted",mask.inverted,false);
            optional_object(&mut m,"placement",v::encode_mask_placement(mask.translation,mask.placement,Rect::from_extent(source.domain))?);
            data.insert("mask".into(),Value::Object(m));
        }
        Ok(record(identity,"capy.occurrence/2",Value::Object(data)))
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
        if let Some((origin,size))=output.frame {data.insert("frame".into(),v::encode_frame(origin,size)?);}
        if output.scale.iter().any(|n|!n.is_finite() || *n<=0.) {return Err("Invalid output scale".into());}
        if output.scale.map(f32::to_bits)!=[1f32.to_bits();2] {data.insert("scale".into(),output.scale.map(f64::from).into());}
        optional_object(&mut data,"sdr",v::encode_sdr(output.sdr)?);
        if let Some(proof)=&output.proof {data.insert("proof".into(),v::encode_proof(proof,|profile|resources.profile(profile))?);}
        Ok(record(identity,"capy.output/1",Value::Object(data)))
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
    for (handle,id,value) in art.definitions.iter() {records.push(encode_change(art,&crate::Edit::Definition(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.effects.iter() {records.push(encode_change(art,&crate::Edit::Effect(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.selections.iter() {records.push(encode_change(art,&crate::Edit::SavedSelection(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.guides.iter() {records.push(encode_change(art,&crate::Edit::Guides(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.occurrences.iter() {records.push(encode_change(art,&crate::Edit::Occurrence(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    for (handle,id,value) in art.outputs.iter() {records.push(encode_change(art,&crate::Edit::Output(crate::RecordChange {handle,id,value:Some(value.clone())}),&mut resources,cancel)?);}
    art.metadata.validate()?;
    for (kind,block) in ["exif","xmp","iptc"].into_iter().zip(art.metadata.blocks()) {if let Some(bytes)=block {resources.bytes("capy.photo-metadata/1",bytes,json!({"kind":kind}))?;}}
    if cancel.load(Ordering::Relaxed) {return Err("Package operation cancelled".into());}
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
            "capy.composition/1"=>{if let Some(handle)=art.compositions.allocated(*identity) {art.compositions.remove(handle);}},
            "capy.stack/1"=>{if let Some(handle)=art.stacks.allocated(*identity) {art.stacks.remove(handle);}},
            "capy.occurrence/2"=>{if let Some(handle)=art.occurrences.allocated(*identity) {art.occurrences.remove(handle);}},
            "capy.paint-source/1"=>{if let Some(handle)=art.paint.allocated(*identity) {art.paint.remove(handle);}},
            "capy.coverage-source/1"=>{if let Some(handle)=art.coverage.allocated(*identity) {art.coverage.remove(handle);}},
            "capy.effect/1"=>{if let Some(handle)=art.effects.allocated(*identity) {art.effects.remove(handle);}},
            "capy.effect-definition/1"=>{if let Some(handle)=art.definitions.allocated(*identity) {art.definitions.remove(handle);}},
            "capy.selection/1"=>{if let Some(handle)=art.selections.allocated(*identity) {art.selections.remove(handle);}},
            "capy.guides/1"=>{if let Some(handle)=art.guides.allocated(*identity) {art.guides.remove(handle);}},
            "capy.output/1"=>{if let Some(handle)=art.outputs.allocated(*identity) {art.outputs.remove(handle);}},
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
        let (kind,data)=payload(record)?; known.push((*identity,kind,data));
        match kind {"capy.composition/1"=>{reserve(&mut art.compositions,*identity,true)?;},"capy.stack/1"=>{reserve(&mut art.stacks,*identity,true)?;},"capy.occurrence/2"=>{reserve(&mut art.occurrences,*identity,true)?;},"capy.paint-source/1"=>{reserve(&mut art.paint,*identity,true)?;},"capy.coverage-source/1"=>{reserve(&mut art.coverage,*identity,true)?;},"capy.effect/1"=>{reserve(&mut art.effects,*identity,true)?;},"capy.effect-definition/1"=>{reserve(&mut art.definitions,*identity,true)?;},"capy.selection/1"=>{reserve(&mut art.selections,*identity,true)?;},"capy.guides/1"=>{reserve(&mut art.guides,*identity,true)?;},"capy.output/1"=>{reserve(&mut art.outputs,*identity,true)?;},_=>unreachable!()}

    }
    for (identity,kind,value) in &known {if *kind=="capy.composition/1" {
        let data=fields(value,&["frame","resolution","color","blend","result"])?;
        let (origin,size)=v::parse_frame(v::required(data,"frame")?)?;
        if size.iter().any(|n|*n>reader.limits.dimension || *n>crate::MAX_EXTENT) {return Err(DecodeError::Unsupported("Composition exceeds dimension admission".into()));}
        let color=data.get("color").map(v::parse_document_color).transpose()?.unwrap_or(DocumentColor {space:crate::color::RgbSpace::Srgb,depth:crate::color::SampleDepth::U8});
        let blend=data.get("blend").map(v::parse_blend_space).transpose()?.unwrap_or(BlendSpace::Linear);
        if color.depth.is_float() && blend!=BlendSpace::Linear {return Err(DecodeError::Unsupported("Float compositions require linear blending".into()));}
        let c=Composition {origin,size,color,blend,resolution:data.get("resolution").map(v::parse_resolution).transpose()?,result:endpoint_handle(&art.stacks,v::required(data,"result")?)?};
        art.compositions.install(art.compositions.allocated(*identity).unwrap(),c)?;
    }}
    let canvas=art.compositions.get(art.root).ok_or("Missing root composition")?.clone();
    for (identity,kind,value) in &known {match *kind {
        "capy.paint-source/1"=> {let data=fields(value,&["domain","tiles","material","original"])?;let domain=dimension(v::required(data,"domain")?,reader)?;
            let raster=decode_raster(data,domain,false,canvas.color,reader)?;let original=data.get("original").map(|v|decode_original(v,reader)).transpose()?;
            if let Some(source)=&original {validate_original_color(source,canvas.color)?;}
            art.paint.install(art.paint.allocated(*identity).unwrap(),PaintSource {domain,raster,original,operations:Arc::default()})?;},
        "capy.coverage-source/1"=> {let data=fields(value,&["domain","tiles","initial","default_coverage"])?;let domain=dimension(v::required(data,"domain")?,reader)?;
            let raster=decode_raster(data,domain,true,canvas.color,reader)?;let initial=data.get("initial").map(|v|selection_records::decode_selection(v,reader)).transpose()?;
            let default_coverage=float_field(data,"default_coverage",1.)?;if !(0. ..=1.).contains(&default_coverage) {return Err("Invalid default coverage".into());}
            art.coverage.install(art.coverage.allocated(*identity).unwrap(),CoverageSource {domain,raster,initial,default_coverage,operations:Arc::default()})?;},
        "capy.effect-definition/1"=> {let definition=effect_records::decode_definition(value,reader)?;art.definitions.install(art.definitions.allocated(*identity).unwrap(),definition)?;},
        "capy.selection/1"=> {let selection=selection_records::decode_selection(value,reader)?;
            art.selections.install(art.selections.allocated(*identity).unwrap(),SavedSelection {selection})?;},
        "capy.guides/1"=> {let guides=decode_guides(value)?;art.guides.install(art.guides.allocated(*identity).unwrap(),guides)?;}, _=>{}
    }}
    for (identity,kind,value) in &known {if *kind=="capy.effect/1" {
        let data=fields(value,&["definition","values","bindings","inputs"])?;
        for key in ["bindings","inputs"] {if let Some(value)=data.get(key) && !value.as_object().ok_or("Expected keyed effect map")?.is_empty() {return Err(DecodeError::Unsupported("Explicit effect inputs or bindings are unsupported".into()));}}
        let definition=handle(&art.definitions,v::required(data,"definition")?)?;let program=&art.definitions.get(definition).ok_or("Missing effect definition")?.program;
        let values=effect_records::decode_values(program,data.get("values").unwrap_or(&json!({})),reader)?;
        art.effects.install(art.effects.allocated(*identity).unwrap(),EffectApplication {definition,values})?;
    }}
    for (identity,kind,value) in &known {match *kind {
        "capy.stack/1"=> {let data=fields(value,&["entries"])?;let entries=data.get("entries").map(|v|list(v)?.iter().map(|value|handle(&art.occurrences,value)).collect::<DecodeResult<Vec<_>>>()).transpose()?.unwrap_or_default();art.stacks.install(art.stacks.allocated(*identity).unwrap(),Stack {entries})?;},
        "capy.occurrence/2"=> {
            let data=fields(value,&["content","name","visible","opacity","blend","locked","alpha_locked","reference","attachment","placement","mask"])?;
            let content=fields(v::required(data,"content")?,&["paint","stack","effect","selection"])?;
            if content.len()!=1 {return Err("Occurrence requires one content alternative".into());}
            let (kind,value)=content.iter().next().unwrap();let content=match kind.as_str() {"paint"=>OccurrenceContent::Paint(handle(&art.paint,value)?),"stack"=>OccurrenceContent::Stack(handle(&art.stacks,value)?),"effect"=>OccurrenceContent::Effect(handle(&art.effects,value)?),"selection"=>OccurrenceContent::Selection(handle(&art.selections,value)?),_=>unreachable!()};
            let bounds=content_bounds(art,&content,canvas.size)?;
            let (translation,placement)=data.get("placement").map(|v|v::parse_placement(v,bounds)).transpose()?.unwrap_or((Point::default(),LayerPlacement::IDENTITY));
            let blend=data.get("blend").map(v::parse_layer_blend).transpose()?.unwrap_or(LayerBlend::Normal);
            if blend==LayerBlend::PassThrough && !matches!(content,OccurrenceContent::Stack(_)) {return Err("Pass through requires a stack".into());}
            let opacity=float_field(data,"opacity",1.)?;if !(0. ..=1.).contains(&opacity) {return Err("Invalid occurrence opacity".into());}
            let mask=data.get("mask").map(|value| {let data=fields(value,&["source","enabled","linked","inverted","placement"])?;let source=handle(&art.coverage,v::required(data,"source")?)?;
                let bounds=Rect::from_extent(art.coverage.get(source).ok_or("Missing coverage source")?.domain);
                let (translation,placement)=data.get("placement").map(|v|v::parse_mask_placement(v,bounds)).transpose()?.unwrap_or((Point::default(),crate::Projective::IDENTITY));
                Ok::<_,DecodeError>(MaskUse {source,enabled:bool_field(data,"enabled",true)?,linked:bool_field(data,"linked",true)?,inverted:bool_field(data,"inverted",false)?,translation,placement})}).transpose()?;
            let attachment=match data.get("attachment").map(v::string).transpose()? {None|Some("none")=>Attachment::None,Some("clip")=>Attachment::Clip,Some("effect")=>Attachment::Effect,Some(name)=>return Err(DecodeError::Unsupported(format!("Unknown occurrence attachment {name}")))};
            let occurrence=Occurrence {content,name:name(data)?,visible:bool_field(data,"visible",true)?,opacity,blend,locked:bool_field(data,"locked",false)?,alpha_locked:bool_field(data,"alpha_locked",false)?,reference:bool_field(data,"reference",false)?,attachment,translation,placement,mask};
            art.occurrences.install(art.occurrences.allocated(*identity).unwrap(),occurrence)?;
        },_=>{}
    }}
    for (identity,kind,value) in &known {if *kind=="capy.output/1" {
        let data=fields(value,&["source","name","context","frame","scale","sdr","proof","representation"])?;
        let composition=endpoint_handle(&art.compositions,v::required(data,"source")?)?;
        let mut context=EvaluationContext::default();
        if let Some(value)=data.get("context") {let data=fields(value,&["effect_phases"])?;
            if let Some(value)=data.get("effect_phases") {let mut seen=BTreeSet::new();for phase in list(value)? {
                let data=fields(phase,&["effect","phase"])?;let effect=handle(&art.effects,v::required(data,"effect")?)?;
                let identity=art.effects.id(effect).unwrap();if !seen.insert(identity) {return Err("Duplicate output phase".into());}
                Arc::make_mut(&mut context.phases).push((effect,v::finite_f32(v::required(data,"phase")?)?));
            }}
        }
        let scale=if let Some(scale)=data.get("scale") {let pair=v::array(scale,2)?;[v::finite_f32(&pair[0])?,v::finite_f32(&pair[1])?]}else{[1.;2]};
        if scale.iter().any(|n|*n<=0.) {return Err("Invalid output scale".into());}
        if let Some(representation)=data.get("representation") {
            let rep=fields(representation,&["member","size","color"])?;
            if v::string(v::required(rep,"member")?)?!="preview.png" {return Err(DecodeError::Unsupported("Unknown output representation".into()));}
            let size=v::parse_size(v::required(rep,"size")?)?;if size.iter().any(|n|*n>1024) {return Err("Oversized output representation".into());}
            if v::string(v::required(rep,"color")?)?!="srgb" {return Err(DecodeError::Unsupported("Unknown representation color".into()));}
        }
        let output=Output {composition,name:name(data)?,context,scale,frame:data.get("frame").map(v::parse_frame).transpose()?,sdr:data.get("sdr").map(v::parse_sdr).transpose()?.unwrap_or(SdrRendition {exposure:0.,contrast:1.,headroom:2.3004484,highlight_color:0.3,balance:0.}),proof:data.get("proof").map(|v|v::parse_proof(v,|profile|reader.profile(profile))).transpose()?};
        art.outputs.install(art.outputs.allocated(*identity).unwrap(),output)?;
    }}
    Ok(())
}

pub fn decode(manifest:&Manifest,reader:&mut ResourceReader<'_>)->DecodeResult<Artwork> {
    decode_with_layout(manifest,reader,None)
}
pub(crate) fn decode_with_layout(manifest:&Manifest,reader:&mut ResourceReader<'_>,layout:Option<&super::transfer::TransferLayout>)->DecodeResult<Artwork> {
    let mut art=Artwork::new([1,1])?;
    art.id=manifest.document; art.compositions=Store::default();art.stacks=Store::default();art.occurrences=Store::default();art.paint=Store::default();art.coverage=Store::default();art.effects=Store::default();art.definitions=Store::default();art.selections=Store::default();art.guides=Store::default();art.outputs=Store::default();
    if let Some(layout)=layout {layout.install(&mut art)?;}
    for (identity,record) in &manifest.objects {
        match record["type"].as_str().ok_or("Missing object type")? {
            "capy.composition/1"=>{reserve(&mut art.compositions,*identity,layout.is_some())?;},
            "capy.stack/1"=>{reserve(&mut art.stacks,*identity,layout.is_some())?;},
            "capy.paint-source/1"=>{reserve(&mut art.paint,*identity,layout.is_some())?;},
            "capy.coverage-source/1"=>{reserve(&mut art.coverage,*identity,layout.is_some())?;},
            "capy.effect-definition/1"=>{reserve(&mut art.definitions,*identity,layout.is_some())?;},
            "capy.effect/1"=>{reserve(&mut art.effects,*identity,layout.is_some())?;},
            "capy.selection/1"=>{reserve(&mut art.selections,*identity,layout.is_some())?;},
            "capy.guides/1"=>{reserve(&mut art.guides,*identity,layout.is_some())?;},
            "capy.occurrence/2"=>{reserve(&mut art.occurrences,*identity,layout.is_some())?;},
            "capy.output/1"=>{reserve(&mut art.outputs,*identity,layout.is_some())?;},
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
    if matches!(manifest.support,Support::Editable){SceneIndex::build(&art)?;}
    Ok(art)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::RasterTile;
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
        let paint=art.paint.insert(PortableId::random(),PaintSource {domain:[37,29],raster:RasterRevision::backed(raster),original:Some(original),operations:Arc::default()}).unwrap();
        let coverage=art.coverage.insert(PortableId::random(),CoverageSource {domain:[37,29],raster:RasterRevision::default(),initial:Some(crate::Selection::polygon(vec![Point {x:0.,y:0.},Point {x:17.,y:0.},Point {x:0.,y:18.}]).unwrap()),default_coverage:0.25,operations:Arc::default()}).unwrap();
        let mut occurrence=Occurrence::new(OccurrenceContent::Paint(paint),"Paint");occurrence.visible=false;occurrence.opacity=0.625;occurrence.translation=Point {x:-0.,y:1.25};occurrence.alpha_locked=true;occurrence.blend=LayerBlend::Multiply;
        occurrence.mask=Some(MaskUse {source:coverage,enabled:false,linked:false,inverted:true,translation:Point {x:1.,y:2.},placement:crate::Projective::IDENTITY});
        let occurrence=art.occurrences.insert(PortableId::random(),occurrence).unwrap();
        let inner=art.stacks.insert(PortableId::random(),Stack {entries:vec![occurrence]}).unwrap();
        let mut group=Occurrence::new(OccurrenceContent::Stack(inner),"Group");group.blend=LayerBlend::PassThrough;
        let group=art.occurrences.insert(PortableId::random(),group).unwrap();
        let stack=art.compositions.get(art.root).unwrap().result;art.stacks.get_mut(stack).unwrap().entries.push(group);
        art.paint.insert(PortableId::random(),PaintSource {domain:[3,5],raster:RasterRevision::default(),original:None,operations:Arc::default()}).unwrap();
        art.selections.insert(PortableId::random(),SavedSelection {selection:crate::Selection::full(),}).unwrap();
        art.selections.insert(PortableId::random(),SavedSelection {selection:crate::Selection::empty(),}).unwrap();
        art.guides.insert(PortableId::random(),Guides {rulers:vec![(PortableId::random(),RulerGeometry::Parallel {start:Point {x:1.,y:2.},end:Point {x:3.,y:4.}})]}).unwrap();
        let program=crate::bundled_effect_catalog().filters()[0].program();let values=crate::EffectInstance::new(program.clone()).values;
        let definition=art.definitions.insert(PortableId::random(),Definition {program}).unwrap();
        let effect=art.effects.insert(PortableId::random(),EffectApplication {definition,values}).unwrap();
        let output=art.outputs.get_mut(art.default_output).unwrap();output.context.phases=vec![(effect,0.75)].into();output.name="Export".into();output.frame=Some((Point {x:-0.,y:1.},[23,17]));output.scale=[1.25,0.75];output.sdr.exposure=0.625;
        output.proof=Some(crate::color::ProofRecipe::new("Print".into(),crate::color::ColorProfile::Builtin(crate::color::RgbSpace::AdobeRgb)));
        art.metadata=Arc::new(PhotoMetadata {exif:Some(Resource::from(vec![7;61])),xmp:Some(Resource::from(vec![11;71])),iptc:None});art
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
