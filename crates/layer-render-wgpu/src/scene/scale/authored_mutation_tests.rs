use super::*;
use layer_core::authored::{Image as AuthoredImage,Output};
use layer_core::authored::vector::{Anchor,Cap,Fill,FillRule,ItemId,Join,Paint,PaintEntry,PathGeometry,PathItem,Record,Stroke,Style,Subpath,VectorObject,WidthKnot,WidthProfile};
use layer_core::color::{DocumentColor,PixelDescriptor,RgbColor,RgbSpace};
use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,RasterWatercolor,TileBlob,TileKey};
use layer_core::{BlendSpace,EffectInstance,EffectValue,GradientDefinition,GradientStop,LayerBlend,TextureSource};

pub(super) struct Spares {paint:PaintHandle,coverage:CoverageHandle,stack:StackHandle,image:AuthoredImage}
pub(super) struct BaseScene {pub name:&'static str,pub artwork:Artwork,spares:Spares}
pub(super) struct Mutation {pub input:&'static str,pub record:String,pub artwork:Artwork}
type Changes<T>=Vec<(&'static str,T)>;

fn replaced<T:PartialEq+std::fmt::Debug>(current:&T,replacement:T)->T {assert_ne!(*current,replacement,"a mutation changes its input");replacement}
fn within<T,U>(changes:Changes<T>,place:impl Fn(T)->U)->impl Iterator<Item=(&'static str,U)> {changes.into_iter().map(move |(input,value)|(input,place(value)))}
fn moved(offset:&[i64;2])->[i64;2] {[offset[0]+24,offset[1]+8]}
fn recolored(color:&RgbColor)->RgbColor {let [r,g,b,a]=color.rgba;RgbColor::new(color.space,[(r+0.5).rem_euclid(1.),g,b,a]).unwrap()}

fn document_color(value:&DocumentColor)->Changes<DocumentColor> {
    let DocumentColor {space,depth}=*value;
    vec![
        ("DocumentColor.space",DocumentColor {space:if space==RgbSpace::Srgb {RgbSpace::DisplayP3}else {RgbSpace::Srgb},depth}),
        ("DocumentColor.depth",DocumentColor {space,depth:if depth==SampleDepth::U8 {SampleDepth::F32}else {SampleDepth::U8}}),
    ]
}

fn composition(value:&Composition,spares:&Spares)->Changes<Composition> {
    let Composition {size,color,blend,resolution:_,result}=value;
    let mut changes=vec![
        ("Composition.size",Composition {size:[size[0]-64,size[1]],..value.clone()}),
        ("Composition.blend",Composition {blend:if *blend==BlendSpace::Linear {BlendSpace::Perceptual}else {BlendSpace::Linear},..value.clone()}),
        ("Composition.result",Composition {result:replaced(result,spares.stack),..value.clone()}),
    ];
    changes.extend(within(document_color(color),|color|Composition {color,..value.clone()}));
    changes
}

fn stack(value:&Stack)->Changes<Stack> {
    let Stack {entries}=value;
    entries.split_first().map(|(_,below)|("Stack.entries",Stack {entries:below.to_vec()})).into_iter().collect()
}

fn mask_use(value:&MaskUse,spares:&Spares)->Changes<MaskUse> {
    let MaskUse {source,enabled,linked,inverted,offset}=value;
    vec![
        ("MaskUse.source",MaskUse {source:replaced(source,spares.coverage),..value.clone()}),
        ("MaskUse.enabled",MaskUse {enabled:!enabled,..value.clone()}),
        ("MaskUse.linked",MaskUse {linked:!linked,..value.clone()}),
        ("MaskUse.inverted",MaskUse {inverted:!inverted,..value.clone()}),
        ("MaskUse.offset",MaskUse {offset:moved(offset),..value.clone()}),
    ]
}

fn occurrence(value:&Occurrence,spares:&Spares)->Changes<Occurrence> {
    let Occurrence {content,name:_,visible,opacity,blend,locked:_,alpha_locked:_,reference:_,attachment,offset,mask}=value;
    let mut attached=value.clone();set_attachment(&mut attached,*attachment==Attachment::None);
    let masked=MaskUse {source:spares.coverage,enabled:true,linked:true,inverted:false,offset:[0;2]};
    let mut changes=vec![
        ("Occurrence.content",Occurrence {content:replaced(content,OccurrenceContent::Paint(spares.paint)),..value.clone()}),
        ("Occurrence.visible",Occurrence {visible:!visible,..value.clone()}),
        ("Occurrence.opacity",Occurrence {opacity:opacity*0.5,..value.clone()}),
        ("Occurrence.blend",Occurrence {blend:if *blend==LayerBlend::Normal {LayerBlend::Multiply}else {LayerBlend::Normal},..value.clone()}),
        ("Occurrence.attachment",attached),
        ("Occurrence.offset",Occurrence {offset:moved(offset),..value.clone()}),
        ("Occurrence.mask",Occurrence {mask:if mask.is_some() {None}else {Some(masked)},..value.clone()}),
    ];
    if let Some(mask)=mask {changes.extend(within(mask_use(mask,spares),|mask|Occurrence {mask:Some(mask),..value.clone()}));}
    changes
}

fn raster_watercolor(value:&RasterWatercolor)->Changes<RasterWatercolor> {
    let RasterWatercolor {wet_edge,burnt_edge,edge_width}=*value;
    vec![
        ("RasterWatercolor.wet_edge",RasterWatercolor {wet_edge:wet_edge*0.5,burnt_edge,edge_width}),
        ("RasterWatercolor.burnt_edge",RasterWatercolor {wet_edge,burnt_edge:burnt_edge*0.5,edge_width}),
        ("RasterWatercolor.edge_width",RasterWatercolor {wet_edge,burnt_edge,edge_width:edge_width*2.}),
    ]
}

fn raster(value:&RasterRevision,plane:RasterPlane,descriptor:PixelDescriptor)->Changes<RasterRevision> {
    let data=value.wait_data().unwrap();let RasterData {tiles,watercolor}=&*data;
    let mut repainted=tiles.clone();
    repainted.insert(TileKey {plane,coordinate:[0;2]},RasterTile::backed(TileBlob::encode(descriptor,&vec![0x60;descriptor.byte_len([PAGE_SIZE;2]).unwrap()]).unwrap()));
    let mut changes=vec![("RasterData.tiles",RasterData {tiles:repainted,watercolor:*watercolor})];
    if let Some(watercolor)=watercolor {changes.extend(within(raster_watercolor(watercolor),|watercolor|RasterData {tiles:tiles.clone(),watercolor:Some(watercolor)}));}
    changes.into_iter().map(|(input,data)|(input,RasterRevision::backed(data))).collect()
}

fn paint_base(value:&PaintBase,spares:&Spares)->Changes<PaintBase> {
    let PaintBase {image,offset,policy:_}=value;
    vec![
        ("PaintBase.image",PaintBase {image:replaced(image,spares.image.clone()),..value.clone()}),
        ("PaintBase.offset",PaintBase {offset:[offset[0]+24,offset[1]+8],..value.clone()}),
    ]
}

fn paint_source(value:&PaintSource,color:DocumentColor,spares:&Spares)->Changes<PaintSource> {
    let PaintSource {color_mode:_,domain:_,raster:pixels,base,operations:_}=value;
    let mut changes=vec![("PaintSource.base",PaintSource {base:if base.is_some() {None}else {Some(PaintBase::new(spares.image.clone()))},..value.clone()})];
    changes.extend(within(raster(pixels,RasterPlane::Color,RasterPlane::Color.descriptor_for(color,value.color_mode)),|raster|PaintSource {raster,..value.clone()}));
    if let Some(base)=base {changes.extend(within(paint_base(base,spares),|base|PaintSource {base:Some(base),..value.clone()}));}
    changes
}

fn coverage_source(value:&CoverageSource)->Changes<CoverageSource> {
    let CoverageSource {domain:_,raster:pixels,default_coverage,operations:_}=value;
    let mut changes=vec![("CoverageSource.default_coverage",CoverageSource {default_coverage:(default_coverage+0.4).rem_euclid(1.),..value.clone()})];
    changes.extend(within(raster(pixels,RasterPlane::Mask,RasterPlane::Mask.descriptor(Default::default())),|raster|CoverageSource {raster,..value.clone()}));
    changes
}

fn image_object(value:&ImageObject,spares:&Spares)->Changes<ImageObject> {
    let ImageObject {image,affine,interpolation,vector:_}=value;
    let [a,b,c,d,x,y]=affine.0;
    vec![
        ("ImageObject.image",ImageObject {image:replaced(image,spares.image.clone()),..value.clone()}),
        ("ImageObject.affine",ImageObject {affine:Affine64([a*1.25,b,c,d*1.25,x+24.,y+8.]),..value.clone()}),
        ("ImageObject.interpolation",ImageObject {interpolation:if *interpolation==ImageInterpolation::Linear {ImageInterpolation::Nearest}else {ImageInterpolation::Linear},..value.clone()}),
    ]
}

fn effect_value(value:&EffectValue)->(&'static str,EffectValue) {
    use EffectValue as V;
    match value {
        V::Number(number)=>("EffectValue::Number",V::Number(number*0.5+0.25)),
        V::Toggle(on)=>("EffectValue::Toggle",V::Toggle(!on)),
        V::Choice(choice)=>("EffectValue::Choice",V::Choice(u32::from(*choice==0))),
        V::Color(color)=>("EffectValue::Color",V::Color(recolored(color))),
        V::Curve(points)=>("EffectValue::Curve",V::Curve(points.iter().map(|[x,y]|[*x,y*0.5]).collect())),
        V::Gradient(gradient)=>("EffectValue::Gradient",V::Gradient(GradientDefinition {interpolation:gradient.interpolation,
            stops:gradient.stops.iter().zip(gradient.stops.iter().rev()).map(|(at,from)|GradientStop {position:at.position,color:from.color}).collect()})),
        V::Lut3d(table)=>("EffectValue::Lut3d",V::Lut3d(if table.is_some() {None}else {Some(layer_core::lut3d::Look::Warm.resource())})),
        V::Texture(texture)=>("EffectValue::Texture",V::Texture(if texture.is_some() {None}
            else {Some(TextureSource::Program(EffectInstance::new(layer_core::bundled_texture_catalog().filters()[0].program())))})),
    }
}

fn effect_spatial_reference(value:&EffectSpatialReference)->Changes<EffectSpatialReference> {
    let EffectSpatialReference {mapping,extent}=*value;let [a,b,c,d,x,y]=mapping.0;
    vec![
        ("EffectSpatialReference.mapping",EffectSpatialReference {mapping:Affine64([a*2.,b,c,d*2.,x+24.,y+8.]),extent}),
        ("EffectSpatialReference.extent",EffectSpatialReference {mapping,extent:[extent[0]*2.,extent[1]*2.]}),
    ]
}

fn effect_application(value:&EffectApplication)->Changes<EffectApplication> {
    let EffectApplication {program,values,spatial}=value;
    let other=crate::tests::fixture(if program.id.as_ref()=="invert" {"desaturate"}else {"invert"}).program();
    let mut changes=vec![("EffectApplication.program",EffectApplication {program:other,..value.clone()})];
    for (index,current) in values.iter().enumerate() {
        let (input,changed)=effect_value(current);let mut values=values.clone();values[index]=changed;
        changes.push((input,EffectApplication {values,..value.clone()}));
    }
    if let Some(spatial)=spatial {changes.extend(within(effect_spatial_reference(spatial),|spatial|EffectApplication {spatial:Some(spatial),..value.clone()}));}
    changes
}

fn paint(value:&Paint)->Changes<Paint> {
    match value {
        Paint::Color {color,unknown:_}=>vec![("Paint::Color.color",Paint::color(recolored(color)))],
        Paint::Opaque(_)=>Vec::new(),
    }
}

fn fill(value:&Fill)->Changes<Fill> {
    let Fill {paint:ink,unknown:_}=value;
    within(paint(ink),|paint|Fill {paint,..value.clone()}).collect()
}

fn stroke(value:&Stroke)->Changes<Stroke> {
    let Stroke {paint:ink,width,cap,join,miter_limit,profile,unknown:_}=value;
    let tapered=Arc::new(WidthProfile::new(vec![WidthKnot::new(0,0.,0.2,1.),WidthKnot::new(0,1.,1.,0.2)]).unwrap());
    let mut changes=vec![
        ("Stroke.width",Stroke {width:width*2.,..value.clone()}),
        ("Stroke.cap",Stroke {cap:if *cap==Cap::Butt {Cap::Round}else {Cap::Butt},..value.clone()}),
        ("Stroke.join",Stroke {join:if *join==Join::Miter {Join::Round}else {Join::Miter},..value.clone()}),
        ("Stroke.miter_limit",Stroke {miter_limit:if *miter_limit>1.5 {1.}else {8.},..value.clone()}),
        ("Stroke.profile",Stroke {profile:if profile.is_some() {None}else {Some(tapered)},..value.clone()}),
    ];
    changes.extend(within(paint(ink),|paint|Stroke {paint,..value.clone()}));
    changes
}

fn style(value:&Style)->Changes<Style> {
    let Style {entries}=value;
    entries.iter().enumerate().flat_map(|(index,entry)| {
        let changes:Changes<PaintEntry>=match entry {
            PaintEntry::Fill(entry)=>within(fill(entry),PaintEntry::Fill).collect(),
            PaintEntry::Stroke(entry)=>within(stroke(entry),PaintEntry::Stroke).collect(),
            PaintEntry::Opaque(_)=>Vec::new(),
        };
        within(changes,move |entry| {let mut entries=entries.to_vec();entries[index]=entry;Style::new(entries)})
    }).collect()
}

fn path_item(value:&PathItem)->Changes<PathItem> {
    let PathItem {id:_,geometry,style:paints,fill_rule,hidden,locked:_,name:_,shape:_,unknown:_}=value;
    let mut changes=vec![
        ("PathItem.geometry",PathItem {geometry:Arc::new(geometry.transformed(|[x,y]|[x+24.,y+8.]).unwrap()),..value.clone()}),
        ("PathItem.fill_rule",PathItem {fill_rule:if *fill_rule==FillRule::NonZero {FillRule::EvenOdd}else {FillRule::NonZero},..value.clone()}),
        ("PathItem.hidden",PathItem {hidden:!hidden,..value.clone()}),
    ];
    changes.extend(within(style(paints),|style|PathItem {style:Arc::new(style),..value.clone()}));
    changes
}

fn vector_object(value:&VectorObject)->Changes<VectorObject> {
    let records:Vec<Record>=value.records().map(|(_,record)|record.clone()).collect();
    records.iter().enumerate().flat_map(|(index,record)| {
        let changes=match record {Record::Path(path)=>path_item(path),Record::Opaque(_)|Record::End=>Vec::new()};
        let records=&records;
        within(changes,move |path| {let mut records=records.clone();records[index]=Record::Path(path);VectorObject::from_records(records)})
    }).collect()
}

fn output(value:&Output)->Changes<Output> {
    let Output {composition:_,name:_,context:_,sdr:_,proof:_}=value;
    Vec::new()
}

fn each<T:Clone>(all:&mut Vec<Mutation>,artwork:&Artwork,store:&Store<T>,spare:Option<Handle<T>>,place:fn(&mut Artwork)->&mut Store<T>,changes:impl Fn(&T)->Changes<T>) {
    for (handle,_,value) in store.iter().filter(|(handle,..)|Some(*handle)!=spare) {
        for (input,changed) in changes(value) {
            let mut artwork=artwork.clone();*place(&mut artwork).get_mut(handle).unwrap()=changed;
            all.push(Mutation {input,record:format!("{} {}",std::any::type_name::<T>().rsplit("::").next().unwrap(),handle.index()),artwork});
        }
    }
}

pub(super) fn mutations(scene:&BaseScene)->Vec<Mutation> {
    let (artwork,spares)=(&scene.artwork,&scene.spares);
    let Artwork {id:_,root:_,compositions,stacks,occurrences,paint,objects,vectors,coverage,effects,selections:_,guides:_,outputs,default_output:_,metadata:_,extensions:_}=artwork;
    let color=compositions.get(artwork.root).unwrap().color;
    let mut all=Vec::new();
    each(&mut all,artwork,compositions,None,|artwork|&mut artwork.compositions,|value|composition(value,spares));
    each(&mut all,artwork,stacks,Some(spares.stack),|artwork|&mut artwork.stacks,stack);
    each(&mut all,artwork,occurrences,None,|artwork|&mut artwork.occurrences,|value|occurrence(value,spares));
    each(&mut all,artwork,paint,Some(spares.paint),|artwork|&mut artwork.paint,|value|paint_source(value,color,spares));
    each(&mut all,artwork,objects,None,|artwork|&mut artwork.objects,|value|image_object(value,spares));
    each(&mut all,artwork,vectors,None,|artwork|&mut artwork.vectors,vector_object);
    each(&mut all,artwork,coverage,Some(spares.coverage),|artwork|&mut artwork.coverage,coverage_source);
    each(&mut all,artwork,effects,None,|artwork|&mut artwork.effects,effect_application);
    each(&mut all,artwork,outputs,None,|artwork|&mut artwork.outputs,output);
    all
}

fn drawing(extent:[u32;2])->Document {Document::from_artwork(Artwork::new(extent).unwrap()).unwrap()}
fn photo(extent:[u32;2],alpha:u8)->Arc<SourceImage> {rgba8_source(extent,|x,y|[(x/3) as u8,(y/2) as u8,80,alpha])}
fn paint_of(doc:&mut Document,owner:OccurrenceHandle)->&mut PaintSource {
    let OccurrenceContent::Paint(paint)=doc.artwork.occurrences.get(owner).unwrap().content else {unreachable!()};
    doc.artwork.paint.get_mut(paint).unwrap()
}
fn catalog_effect(doc:&mut Document,program:&str,values:&[(&str,EffectValue)])->OccurrenceHandle {
    let mut effect=EffectInstance::new(crate::tests::fixture(program).program());
    for (key,value) in values {effect.set(key,value.clone()).unwrap();}
    effect_occurrence(doc,effect,program)
}
fn finished(name:&'static str,mut doc:Document)->BaseScene {
    let extent=doc.composition().size;
    let image=AuthoredImage::new(rgba8_source([48;2],|x,y|[(x*5) as u8,200,(y*5) as u8,255]));
    let spares=Spares {
        paint:doc.artwork.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:extent,raster:Default::default(),
            base:Some(PaintBase::new(rgba8_source(extent,|x,y|[(x^y) as u8,90,200,255]).into())),operations:Arc::default()}).unwrap(),
        coverage:doc.artwork.coverage.insert(PortableId::random(),CoverageSource {domain:extent,raster:Default::default(),default_coverage:0.35,operations:Arc::default()}).unwrap(),
        stack:doc.artwork.stacks.insert(PortableId::random(),Stack::default()).unwrap(),image,
    };
    BaseScene {name,artwork:doc.artwork,spares}
}

fn layers()->BaseScene {
    let extent=[512,256];let mut doc=drawing(extent);let color=doc.composition().color;
    let pixels=|descriptor:PixelDescriptor,pixel:&dyn Fn(u32,u32)->Vec<u8>|RasterTile::backed(TileBlob::encode(descriptor,
        &(0..PAGE_SIZE*PAGE_SIZE).flat_map(|index|pixel(index%PAGE_SIZE,index/PAGE_SIZE)).collect::<Vec<u8>>()).unwrap());
    let tiles=|plane:RasterPlane,descriptor:PixelDescriptor,pixel:&dyn Fn(u32,u32)->Vec<u8>|[[0,0],[1,0]].map(|coordinate|(TileKey {plane,coordinate},pixels(descriptor,pixel)));
    let striped=|x:u32,y:u32|if (x+y)/32%2==0 {220}else {0};
    let backdrop=paint_occurrence(&mut doc,"Backdrop",Some(photo(extent,255)));
    let painted=paint_occurrence(&mut doc,"Painted",None);
    paint_of(&mut doc,painted).raster=RasterRevision::backed(RasterData {watercolor:None,
        tiles:tiles(RasterPlane::Color,color.paint_descriptor(),&|x,y|vec![40,x as u8,y as u8,striped(x,y)]).into()});
    doc.artwork.occurrences.get_mut(painted).unwrap().offset=[8,4];
    let washed=paint_occurrence(&mut doc,"Washed",None);
    paint_of(&mut doc,washed).raster=RasterRevision::backed(RasterData {watercolor:Some(RasterWatercolor {wet_edge:0.9,burnt_edge:0.6,edge_width:8.}),
        tiles:tiles(RasterPlane::Color,color.paint_descriptor(),&|x,y|vec![30,90,200,if (x/64+y/64)%2==0 {160}else {0}]).into_iter()
            .chain(tiles(RasterPlane::WatercolorWetness,RasterPlane::WatercolorWetness.descriptor(color),&|x,y|vec![if (x/64+y/64)%2==0 {127}else {0}])).collect()});
    let clipped=paint_occurrence(&mut doc,"Clipped",Some(rgba8_source([160,120],|x,y|[250,x as u8,(y*2) as u8,230])));
    paint_of(&mut doc,clipped).base.as_mut().unwrap().offset=[24,16];
    let layer=doc.artwork.occurrences.get_mut(clipped).unwrap();layer.offset=[40,24];layer.opacity=0.9;
    let group=stack_occurrence(&mut doc,"Group",vec![clipped,painted]);
    doc.artwork.occurrences.get_mut(group).unwrap().opacity=0.85;
    set_attachment(doc.artwork.occurrences.get_mut(clipped).unwrap(),true);
    set_root_entries(&mut doc,vec![washed,group,backdrop]);
    let outline=|points:[[f32;2];4]|layer_core::Selection::polygon(points.map(|[x,y]|layer_core::Point {x,y}).to_vec()).unwrap();
    let mask=coverage_mask(&mut doc,clipped,[8,4],Some(outline([[60.,30.],[200.,30.],[200.,150.],[60.,150.]])));
    doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.25;
    let mask=coverage_mask(&mut doc,painted,[0;2],Some(outline([[100.,40.],[470.,40.],[470.,200.],[100.,200.]])));
    doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.25;
    finished("layers",doc)
}

fn effects()->BaseScene {
    use EffectValue as V;
    let extent=[512,256];let mut doc=drawing(extent);
    let subject=paint_occurrence(&mut doc,"Photo",Some(photo(extent,200)));
    let floor=catalog_effect(&mut doc,"gradient_fill",&[]);
    let curved=catalog_effect(&mut doc,"curves",&[("rgb",V::Curve(vec![[0.,0.1],[0.5,0.7],[1.,0.9]]))]);
    set_attachment(doc.artwork.occurrences.get_mut(curved).unwrap(),true);
    let looked=catalog_effect(&mut doc,"color_lookup",&[("resource",V::Lut3d(Some(layer_core::lut3d::Look::Cool.resource())))]);
    let filtered=catalog_effect(&mut doc,"photo_filter",&[]);
    let inverted=catalog_effect(&mut doc,"invert",&[]);
    doc.artwork.occurrences.get_mut(inverted).unwrap().opacity=0.5;
    let blurred=catalog_effect(&mut doc,"gaussian_blur",&[("sigma",V::Number(3.))]);
    set_root_entries(&mut doc,vec![blurred,inverted,filtered,looked,curved,subject,floor]);
    let mask=coverage_mask(&mut doc,filtered,[0;2],None);
    doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.5;
    finished("effects",doc)
}

fn objects()->BaseScene {
    use EffectValue as V;
    let extent=[512,256];let mut doc=drawing(extent);
    let mut placed=ImageObject::new(AuthoredImage::new(rgba8_source([200,120],|x,y|[x as u8,(y*2) as u8,(x^y) as u8,255])));
    placed.affine=Affine64([1.3,0.,0.,1.3,180.5,30.25]);
    let object=doc.artwork.objects.insert(PortableId::random(),placed).unwrap();
    let image=doc.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Image")).unwrap();
    doc.artwork.occurrences.get_mut(image).unwrap().opacity=0.9;
    let outline=|points:&[[f64;2]],closed|Subpath::new(&points.iter().copied().map(Anchor::corner).collect::<Vec<_>>(),closed);
    let geometry=PathGeometry::new(vec![
        outline(&[[30.,30.],[300.,40.],[280.,200.],[40.,220.]],true),outline(&[[90.,80.],[200.,90.],[190.,160.],[100.,170.]],true),
        outline(&[[230.,230.],[330.,120.],[360.,235.],[480.,60.]],false),
    ]).unwrap();
    let ink=|rgba|Paint::color(RgbColor::new(RgbSpace::Srgb,rgba).unwrap());
    let paints=Style::new(vec![PaintEntry::Fill(Fill {paint:ink([0.9,0.3,0.2,1.]),unknown:None}),PaintEntry::Stroke(Stroke::new(ink([0.1,0.2,0.7,1.]),14.))]);
    let shapes=doc.artwork.vectors.insert(PortableId::random(),VectorObject::from_records(vec![Record::Path(PathItem::new(ItemId(1),Arc::new(geometry),Arc::new(paints)))])).unwrap();
    let vector=doc.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Vector(shapes),"Shapes")).unwrap();
    let paper=catalog_effect(&mut doc,layer_core::paper::PAPER_PROGRAM,&[("relief",V::Number(100.)),("through_paint",V::Number(50.))]);
    set_root_entries(&mut doc,vec![vector,image,paper]);
    finished("objects",doc)
}

pub(super) fn base_scenes()->Vec<BaseScene> {vec![layers(),effects(),objects()]}
