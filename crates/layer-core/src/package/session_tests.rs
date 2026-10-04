use crate::package::{session::{PreparedSession, open}, session_transfer::{PreparedSessionTransfer, SessionTransferReceiver}, ImmutableBacking, MAX_RANGE_BYTES};
use crate::{authored::*, color::{ColorProfile, RgbColor, RgbSpace, ProofRecipe, hdr::SdrRendition},
    raster::{RasterData, RasterPlane, RasterRevision, RasterTile, RasterWatercolor, TileBlob, TileKey, TILE_SIZE},
    BlendSpace, Document, DocumentNames, Edit, Editor, EffectInstance, EffectValue, ImageResolution,
    Interpolation, LayerBlend, LayerPlacement, Lut3d, MeshMap, PhotoMetadata, Point, Projective,
    ProjectLimits, Rect, RulerGeometry, Selection, SelectionMaskProperties, SelectionPixels};
use serde_json::json;
use std::{collections::BTreeSet, sync::{Arc, atomic::AtomicBool}};

fn raster(plane:RasterPlane,marker:u8,material:bool)->RasterRevision {
    let planes=if material {vec![plane,RasterPlane::Wetness,RasterPlane::WatercolorWetness]}else{vec![plane]};
    RasterRevision::backed(RasterData {
        tiles:planes.into_iter().map(|plane|{let descriptor=plane.descriptor(Default::default());
            (TileKey {plane,coordinate:[0,0]},RasterTile::backed(TileBlob::encode(descriptor,
                &vec![marker;descriptor.byte_len([TILE_SIZE;2]).unwrap()]).unwrap()))}).collect(),
        watercolor:material.then_some(RasterWatercolor {wet_edge:0.25,burnt_edge:0.5,edge_width:2.}),
    })
}
fn pixels()->Selection {
    Selection::pixels(Arc::new(SelectionPixels::new([9,2],[0,0,9,2],vec![0x43214321,4,0x12341234,1]).unwrap()))
}
fn lut(marker:f32)->Arc<Lut3d> {
    Arc::new(Lut3d::from_samples(2,[[0.;3],[1.;3]],Arc::from("Lookup 色"),vec![[marker;3];8].into()).unwrap())
}
fn fixture()->Editor {
    let initial=Document::new(PortableId::random(),19,11,DocumentNames {paint:"Ink 色".into(),paper:"Paper".into()});
    let mut art=initial.artwork.clone();
    let paint=art.paint.iter().next().unwrap().0;
    let ink=initial.working.occurrence.unwrap();
    let original=crate::color::source::rgba8_source([19,11],|x,y|[x as u8,y as u8,73,255]);
    *art.paint.get_mut(paint).unwrap()=PaintSource {domain:[19,11],raster:raster(RasterPlane::Color,23,true),original:Some(original),operations:Arc::default()};
    let coverage=art.coverage.insert(PortableId::random(),CoverageSource {domain:[19,11],raster:raster(RasterPlane::Mask,127,false),
        initial:Some(Selection::polygon(vec![Point{x:1.,y:2.},Point{x:15.,y:2.},Point{x:1.,y:9.}]).unwrap()),default_coverage:0.375,operations:Arc::default()}).unwrap();
    *art.occurrences.get_mut(ink).unwrap()=Occurrence {content:OccurrenceContent::Paint(paint),name:"Ink 色".into(),visible:false,
        opacity:0.625,blend:LayerBlend::Multiply,locked:true,alpha_locked:true,reference:true,attachment:Attachment::None,isolated_blend:LayerBlend::Screen,
        translation:Point{x:2.,y:3.},placement:LayerPlacement {outer:Projective([1.,0.,1.,0.,1.,2.,0.,0.,1.]),
            mesh:Some(Arc::new(MeshMap::fit(Rect::from_extent([19,11]),[1,1],|p|Some(Point{x:p.x+0.01*p.y*p.y,y:p.y})).unwrap())),interpolation:Interpolation::Bicubic},
        mask:Some(MaskUse {source:coverage,enabled:false,linked:false,inverted:true,translation:Point{x:3.,y:1.},placement:Projective([1.,0.,2.,0.,1.,0.,0.,0.,1.])})};
    let saved=art.selections.insert(PortableId::random(),SavedSelection {selection:pixels(),display:SelectionMaskProperties {
        color:RgbColor {space:RgbSpace::DisplayP3,rgba:[0.25,0.5,0.75,1.],linear_rgb:None},opacity:0.75}}).unwrap();
    let selection=art.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Selection(saved),"Selected 色")).unwrap();
    let program=crate::bundled_effect_catalog().get("color_lookup").unwrap().program();
    let mut effect=EffectInstance::new(program.clone());effect.set("resource",EffectValue::Lut3d(Some(lut(0.25)))).unwrap();
    let definition=art.definitions.insert(PortableId::random(),Definition {program}).unwrap();
    let lookup=art.effects.insert(PortableId::random(),EffectApplication {definition,values:effect.values,domain:[19,11]}).unwrap();
    let lookup_occurrence=art.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(lookup),"Lookup 色")).unwrap();
    let stack=art.compositions.get(art.root).unwrap().result;
    let mut entries=art.stacks.get(stack).unwrap().entries.clone();entries.extend([lookup_occurrence,selection]);
    *art.stacks.get_mut(stack).unwrap()=Stack {entries};
    art.guides.insert(PortableId::random(),Guides {rulers:vec![(PortableId::random(),RulerGeometry::Parallel {start:Point{x:1.,y:2.},end:Point{x:9.,y:3.}})]}).unwrap();
    art.metadata=Arc::new(PhotoMetadata {exif:Some(Resource::from(vec![11;53])),xmp:Some(Resource::from(vec![13;67])),iptc:Some(Resource::from(vec![17;31]))});
    let mut document=Document::from_artwork(art).unwrap();
    document.working=WorkingState {generation:0,selection:Some(pixels()),selection_visibility:[(selection,false)].into(),
        occurrence:Some(ink),target:Some(SourceTarget::Coverage(coverage)),inspect_mask:Some(ink)};
    Editor::new(document)
}
fn edit_family(edit:&Edit)->&'static str {
    match edit {
        Edit::Composition(_)=>"composition",Edit::Stack(_)=>"stack",Edit::Occurrence(_)=>"occurrence",
        Edit::Paint(_)=>"paint",Edit::Coverage(_)=>"coverage",Edit::Effect(_)=>"effect",Edit::Definition(_)=>"definition",
        Edit::SavedSelection(_)=>"selection",Edit::Guides(_)=>"guides",Edit::Output(_)=>"output",Edit::Working(_)=>"working",
        Edit::Batch(_)=>"batch",Edit::SetRaster{..}=>"raster",
    }
}
fn perform_all(editor:&mut Editor) {
    let mut families=BTreeSet::new();
    macro_rules! perform {($edit:expr)=>{{let edit=$edit;families.insert(edit_family(&edit));editor.perform(edit).unwrap();}};}
    macro_rules! change {
        ($store:ident,$variant:ident,$handle:expr,$value:expr)=>{{
            let edit=Edit::$variant(RecordChange::replace(&editor.document().artwork.$store,$handle,Some($value)).unwrap());
            perform!(edit);
        }};
    }
    let art=&editor.document().artwork;
    let root=art.root;let stack=art.compositions.get(root).unwrap().result;
    let ink=editor.document().working.occurrence.unwrap();
    let paint=art.paint.iter().next().unwrap().0;let coverage=art.coverage.iter().next().unwrap().0;
    let saved=art.selections.iter().next().unwrap().0;let guides=art.guides.iter().next().unwrap().0;
    let lookup=art.effects.iter().find(|(_,_,e)|art.definitions.get(e.definition).unwrap().program.id.as_ref()=="color_lookup").unwrap().0;
    let definition=art.effects.get(lookup).unwrap().definition;
    let output=art.default_output;
    let composition=art.compositions.get(root).unwrap();
    let composition=Composition {size:[23,17],origin:Point{x:-2.,y:3.},color:composition.color,blend:BlendSpace::Linear,
        resolution:Some(ImageResolution::ppi(240)),result:stack};
    change!(compositions,Composition,root,composition);
    let mut entries=editor.document().artwork.stacks.get(stack).unwrap().entries.clone();entries.rotate_left(1);
    change!(stacks,Stack,stack,Stack {entries});
    let mut occurrence=editor.document().artwork.occurrences.get(ink).unwrap().clone();occurrence.name="Renamed 🖌".into();occurrence.opacity=0.375;
    change!(occurrences,Occurrence,ink,occurrence);
    change!(paint,Paint,paint,PaintSource {domain:[19,11],raster:raster(RasterPlane::Color,37,true),
        original:Some(crate::color::source::rgba8_source([19,11],|x,y|[y as u8,x as u8,97,255])),operations:Arc::default()});
    change!(coverage,Coverage,coverage,CoverageSource {domain:[19,11],raster:raster(RasterPlane::Mask,79,false),initial:Some(pixels()),default_coverage:0.625,operations:Arc::default()});
    let mut program=crate::effect_catalog::custom_program("color_lookup");
    Arc::make_mut(&mut Arc::make_mut(&mut program).parameters).iter_mut().find(|parameter|parameter.key.as_ref()=="resource").unwrap().dimension=Dimension::Normalized;
    change!(definitions,Definition,definition,Definition {program:program.clone()});
    let mut effect=EffectInstance::new(program);effect.set("resource",EffectValue::Lut3d(Some(lut(0.75)))).unwrap();
    change!(effects,Effect,lookup,EffectApplication {definition,values:effect.values,domain:[19,11]});
    change!(selections,SavedSelection,saved,SavedSelection {selection:Selection::polygon(vec![Point{x:2.,y:1.},Point{x:17.,y:1.},Point{x:17.,y:8.}]).unwrap(),
        display:SelectionMaskProperties {color:RgbColor {space:RgbSpace::Srgb,rgba:[0.75,0.25,0.5,1.],linear_rgb:None},opacity:0.25}});
    change!(guides,Guides,guides,Guides {rulers:vec![(PortableId::random(),RulerGeometry::Radial {center:Point{x:7.,y:5.}})]});
    let mut proof=ProofRecipe::new("Print 色".into(),ColorProfile::Builtin(RgbSpace::AdobeRgb));proof.simulate_paper=true;
    change!(outputs,Output,output,Output {composition:root,name:"Output 色".into(),context:EvaluationContext {elapsed:3.25,phases:Arc::new(vec![(lookup,0.625)])},
        frame:Some((Point{x:-1.,y:2.},[17,9])),scale:[1.5,0.75],sdr:SdrRendition {exposure:0.5,contrast:1.25,headroom:2.,highlight_color:0.25,balance:0.125},proof:Some(proof)});
    let mut working=editor.document().working.clone();working.selection=Some(Selection::polygon(vec![Point{x:1.,y:1.},Point{x:8.,y:1.},Point{x:8.,y:7.}]).unwrap());
    perform!(Edit::Working(working));
    perform!(Edit::SetRaster {target:SourceTarget::Paint(paint),revision:raster(RasterPlane::Color,59,true)});
    let art=&editor.document().artwork;let mut o=art.occurrences.get(ink).unwrap().clone();o.visible=true;o.locked=false;
    let mut working=editor.document().working.clone();working.selection=None;
    perform!(Edit::Batch(vec![Edit::Occurrence(RecordChange::replace(&art.occurrences,ink,Some(o)).unwrap()),Edit::Working(working)]));
    assert_eq!(families,["composition","stack","occurrence","paint","coverage","effect","definition","selection","guides","output","working","raster","batch"].into());
}
fn assert_raster(a:&RasterRevision,b:&RasterRevision) {
    let a=a.wait_data().unwrap();let b=b.wait_data().unwrap();
    assert_eq!(a.watercolor,b.watercolor);assert_eq!(a.tiles.len(),b.tiles.len());
    for (key,tile) in &a.tiles {
        let a=tile.wait_backing().unwrap();let b=b.tiles[key].wait_backing().unwrap();
        assert_eq!(a.descriptor,b.descriptor);assert_eq!(a.resource_id(),b.resource_id());assert_eq!(a.decode().unwrap(),b.decode().unwrap());
    }
}
fn assert_shader(a:&crate::EffectShader,b:&crate::EffectShader) {
    let a=a.sources().unwrap();let b=b.sources().unwrap();assert_eq!(a.len(),b.len());
    for (a,b) in a.iter().zip(b) {assert_eq!(a.id(),b.id());assert_eq!(a.as_ref(),b.as_ref());}
}
fn normalize_shader(a:&mut crate::EffectShader,b:&mut crate::EffectShader) {
    assert_shader(a,b);
    for shader in [a,b] {
        if let crate::EffectShader::Linked {sources}=shader && sources.len()==1 { *shader=crate::EffectShader::Code(sources[0].clone()); }
    }
}
fn assert_editor(expected:&Editor,actual:&Editor) {
    assert_eq!(expected.checkpoint(),actual.checkpoint());assert_eq!(expected.can_undo(),actual.can_undo());assert_eq!(expected.can_redo(),actual.can_redo());
    assert_eq!(expected.document().working,actual.document().working);assert_eq!(expected.document().next_stroke_id(),actual.document().next_stroke_id());
    let mut a=expected.document().artwork.clone();let mut b=actual.document().artwork.clone();
    for (handle,id,source) in expected.document().artwork.paint.iter() {
        let other=actual.document().artwork.paint.get(handle).unwrap();assert_eq!(actual.document().artwork.paint.id(handle),Some(id));
        assert_raster(&source.raster,&other.raster);
        if let Some(original)=&source.original {let restored=other.original.as_ref().unwrap();
            for (coordinate,tile) in &original.tiles {let other=&restored.tiles[coordinate];assert_eq!(tile.resource_id(),other.resource_id());assert_eq!(tile.decode().unwrap(),other.decode().unwrap());}}
        b.paint.get_mut(handle).unwrap().raster=source.raster.clone();
    }
    for (handle,_,source) in expected.document().artwork.coverage.iter() {
        assert_raster(&source.raster,&actual.document().artwork.coverage.get(handle).unwrap().raster);
        b.coverage.get_mut(handle).unwrap().raster=source.raster.clone();
    }
    for (handle,id,_) in expected.document().artwork.definitions.iter() {
        assert_eq!(b.definitions.id(handle),Some(id));
        let left=Arc::make_mut(&mut a.definitions.get_mut(handle).unwrap().program);
        let right=Arc::make_mut(&mut b.definitions.get_mut(handle).unwrap().program);
        normalize_shader(&mut left.wgsl,&mut right.wgsl);assert_eq!(left.lookups.len(),right.lookups.len());
        for (a,b) in Arc::make_mut(&mut left.lookups).iter_mut().zip(Arc::make_mut(&mut right.lookups).iter_mut()) {normalize_shader(&mut a.wgsl,&mut b.wgsl);}
    }
    assert_eq!(a,b);
}
fn transfer(editor:&Editor)->Editor {
    let cancel=AtomicBool::new(false);let metadata=json!({"tab":"Drawing 色"});
    let capture=editor.capture_session(editor.capture(11,editor.document().output().context.clone()).unwrap()).unwrap();
    let transfer=PreparedSessionTransfer::capture(&capture,metadata.clone(),&cancel).unwrap();
    let descriptor=serde_json::from_slice(&serde_json::to_vec(transfer.descriptor()).unwrap()).unwrap();
    let mut receiver=SessionTransferReceiver::new(descriptor,ProjectLimits::default()).unwrap();
    for index in 0..transfer.payload_count() {let length=transfer.payload_len(index).unwrap();let mut offset=0;
        while offset<length {let count=(length-offset).min(MAX_RANGE_BYTES as u64) as usize;
            receiver.push_chunk(index,&transfer.read_chunk(index,offset,count).unwrap()).unwrap();offset+=count as u64;}}
    let restored=receiver.finish().unwrap().adopt_verified(ProjectLimits::default(),&cancel).unwrap();
    assert_eq!(restored.metadata.value,metadata);assert_eq!(restored.editor.document().working.generation,editor.document().working.generation);
    assert_editor(editor,&restored.editor);restored.editor
}
fn reopen(editor:&Editor)->Editor {
    let transferred=transfer(editor);let editor=&transferred;
    let cancel=AtomicBool::new(false);
    let capture=editor.capture_session(editor.capture(11,editor.document().output().context.clone()).unwrap()).unwrap();
    let metadata=json!({"destination":"file:///private/色.capy","saved_checkpoint":3,"camera":{"zoom":1.75,"rotation":0.25}});
    let prepared=PreparedSession::prepare(&capture,metadata.clone(),&cancel).unwrap();
    let mut bytes=Vec::new();prepared.write(&mut bytes,&cancel).unwrap();
    let loaded=open(ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap(),ProjectLimits::default(),&cancel).unwrap();
    assert_eq!(loaded.metadata.value,metadata);assert_eq!(loaded.editor.document().working.generation,editor.document().working.generation);
    assert_editor(editor,&loaded.editor);transfer(&loaded.editor)
}
#[test]
fn every_record_and_raster_edit_restores_semantics_across_undo_redo_and_branching() {
    let mut original=fixture();original.allocate_stroke_id();original.allocate_stroke_id();perform_all(&mut original);
    for _ in 0..5 {assert!(original.undo().unwrap());}
    let mut restored=reopen(&original);assert_editor(&original,&restored);
    for redo in [true,true,false,false,true,true,true,true,true,false,false] {
        assert_eq!(if redo {original.redo()}else{original.undo()}.unwrap(),if redo {restored.redo()}else{restored.undo()}.unwrap());
        assert_editor(&original,&restored);
    }
    while original.undo().unwrap() {assert!(restored.undo().unwrap());assert_editor(&original,&restored);}
    assert!(!restored.undo().unwrap());
    while original.redo().unwrap() {assert!(restored.redo().unwrap());assert_editor(&original,&restored);}
    assert!(!restored.redo().unwrap());
    let mut restored=reopen(&restored);assert_editor(&original,&restored);
    assert!(original.undo().unwrap());assert!(restored.undo().unwrap());
    let saved_checkpoint=original.checkpoint();
    for editor in [&mut original,&mut restored] {
        let art=&editor.document().artwork;let h=editor.document().working.occurrence.unwrap();let mut occurrence=art.occurrences.get(h).unwrap().clone();occurrence.name="Branch 色".into();
        editor.perform(Edit::Occurrence(RecordChange::replace(&art.occurrences,h,Some(occurrence)).unwrap())).unwrap();
        assert!(!editor.can_redo());assert!(editor.checkpoint()>saved_checkpoint);
    }
    assert_editor(&original,&restored);
    assert_eq!(original.allocate_stroke_id(),restored.allocate_stroke_id());
}

#[test]
fn dehaze_definition_and_shared_shader_modules_survive_restart_and_history() {
    let mut original=fixture();
    let effect=original.document().artwork.effects.iter().find(|(_,_,effect)|original.document().artwork.definitions.get(effect.definition).unwrap().program.id.as_ref()=="color_lookup").unwrap().0;
    let definition=original.document().artwork.effects.get(effect).unwrap().definition;
    let program=crate::bundled_effect_catalog().get("dehaze").unwrap().program();
    assert_eq!(program.analysis(),Some(crate::EffectAnalysisKind::Dehaze));
    assert_eq!(program.wgsl.sources().unwrap().len(),3);
    let mut instance=EffectInstance::new(program.clone());instance.set("amount",EffectValue::Number(-35.)).unwrap();
    original.perform(Edit::Batch(vec![
        Edit::Definition(RecordChange::replace(&original.document().artwork.definitions,definition,Some(Definition {program})).unwrap()),
        Edit::Effect(RecordChange::replace(&original.document().artwork.effects,effect,Some(EffectApplication {definition,values:instance.values,domain:[19,11]})).unwrap()),
    ])).unwrap();
    let mut restored=reopen(&original);
    assert!(original.undo().unwrap());assert!(restored.undo().unwrap());assert_editor(&original,&restored);
    let mut restored=reopen(&restored);
    assert!(original.redo().unwrap());assert!(restored.redo().unwrap());assert_editor(&original,&restored);
    assert_eq!(restored.document().artwork.definitions.get(definition).unwrap().program.analysis(),Some(crate::EffectAnalysisKind::Dehaze));
}

#[test]
fn gradient_defaults_interpolation_hdr_colors_and_stops_survive_restart_and_history() {
    let gradient=|interpolation|crate::GradientDefinition {
        stops:vec![
            crate::GradientStop {position:0.,color:RgbColor::from_linear(RgbSpace::DisplayP3,[0.1,0.2,1.8,0.25]).unwrap()},
            crate::GradientStop {position:0.375,color:RgbColor::new(RgbSpace::AdobeRgb,[0.7,0.3,0.2,0.625]).unwrap()},
            crate::GradientStop {position:1.,color:RgbColor::from_linear(RgbSpace::Srgb,[1.25,0.4,0.05,1.]).unwrap()},
        ],interpolation,
    };
    let mut original=fixture();
    let art=&original.document().artwork;
    let effect=art.effects.iter().find(|(_,_,effect)|art.definitions.get(effect.definition).unwrap().program.id.as_ref()=="color_lookup").unwrap().0;
    let definition=art.effects.get(effect).unwrap().definition;
    let mut program=crate::effect_catalog::custom_program("gradient_map");
    Arc::make_mut(&mut Arc::make_mut(&mut program).parameters).iter_mut().find(|parameter|parameter.key.as_ref()=="gradient").unwrap()
        .default=EffectValue::Gradient(gradient(crate::ColorMixSpace::Classic));
    let mut instance=EffectInstance::new(program.clone());
    instance.set("gradient",EffectValue::Gradient(gradient(crate::ColorMixSpace::LinearRgb))).unwrap();
    original.perform(Edit::Batch(vec![
        Edit::Definition(RecordChange::replace(&art.definitions,definition,Some(Definition {program:program.clone()})).unwrap()),
        Edit::Effect(RecordChange::replace(&art.effects,effect,Some(EffectApplication {definition,values:instance.values,domain:[19,11]})).unwrap()),
    ])).unwrap();
    for interpolation in [crate::ColorMixSpace::Oklab,crate::ColorMixSpace::Classic] {
        let mut value=gradient(interpolation);value.reverse();
        let mut instance=EffectInstance::new(program.clone());instance.set("gradient",EffectValue::Gradient(value)).unwrap();
        original.perform(Edit::Effect(RecordChange::replace(&original.document().artwork.effects,effect,
            Some(EffectApplication {definition,values:instance.values,domain:[19,11]})).unwrap())).unwrap();
    }
    assert!(original.undo().unwrap());
    let mut restored=reopen(&original);
    while original.undo().unwrap() {assert!(restored.undo().unwrap());assert_editor(&original,&restored);}
    assert!(!restored.undo().unwrap());
    let mut restored=reopen(&restored);
    while original.redo().unwrap() {assert!(restored.redo().unwrap());assert_editor(&original,&restored);}
    assert!(!restored.redo().unwrap());
    assert!(original.undo().unwrap());assert!(restored.undo().unwrap());
    for editor in [&mut original,&mut restored] {
        let instance=EffectInstance::new(program.clone());
        editor.perform(Edit::Effect(RecordChange::replace(&editor.document().artwork.effects,effect,
            Some(EffectApplication {definition,values:instance.values,domain:[19,11]})).unwrap())).unwrap();
        assert!(!editor.can_redo());
    }
    assert_editor(&original,&restored);
    let restored=reopen(&restored);
    assert_editor(&original,&restored);
}

#[test]
fn dependent_effect_definition_and_output_phase_removal_survive_restart_and_undo() {
    let mut original=fixture();
    let art=&mut original.document.artwork;
    let effect=art.effects.iter().find(|(_,_,effect)|art.definitions.get(effect.definition).unwrap().program.id.as_ref()=="color_lookup").unwrap().0;
    let definition=art.effects.get(effect).unwrap().definition;
    let occurrence=art.occurrences.iter().find(|(_,_,occurrence)|occurrence.content==OccurrenceContent::Effect(effect)).unwrap().0;
    art.outputs.get_mut(art.default_output).unwrap().context=EvaluationContext {elapsed:7.25,phases:vec![(effect,0.625)].into()};
    original.perform(original.document().delete_layers_edit(&[occurrence]).unwrap()).unwrap();
    assert!(original.document().artwork.definitions.get(definition).is_none());
    assert!(original.document().output().context.phases.is_empty());
    let mut restored=reopen(&original);
    assert!(original.undo().unwrap());assert!(restored.undo().unwrap());assert_editor(&original,&restored);
    assert_eq!(restored.document().output().context.phases.as_slice(),&[(effect,0.625)]);
    let mut restored=reopen(&restored);
    assert!(original.redo().unwrap());assert!(restored.redo().unwrap());assert_editor(&original,&restored);
    assert!(restored.document().artwork.definitions.get(definition).is_none());
    assert!(restored.document().output().context.phases.is_empty());
}

#[test]
fn attachment_chains_common_clip_bases_and_isolated_blends_survive_restart_and_history() {
    use crate::operation_test_support as f;
    let mut document=f::document([32,32],&["Top","Curves","Saved","Blur","Shade","Base blur","Base","Backdrop","Group","Member"]);
    f::effect(&mut document,"Curves","exposure");f::effect(&mut document,"Blur","gaussian_blur");f::effect(&mut document,"Base blur","gaussian_blur");
    f::saved(&mut document,"Saved",Selection::empty());let group=f::nest(&mut document,"Group",&["Member"]);
    let top=f::id(&document,"Top");let shade=f::id(&document,"Shade");let base=f::id(&document,"Base");
    let blur=f::id(&document,"Blur");let curves=f::id(&document,"Curves");
    let mut original=Editor::new(document);
    for blend in [LayerBlend::Multiply,LayerBlend::PassThrough] {
        original.perform(original.document().group_blend_edit(group,blend).unwrap()).unwrap();
    }
    for name in ["Base blur","Blur","Curves","Shade","Top"] {
        let handle=f::id(original.document(),name);
        original.perform(original.document().attachment_edit(handle,true,false).unwrap()).unwrap();
    }
    original.perform(original.document().attachment_edit(curves,false,false).unwrap()).unwrap();original.undo().unwrap();
    let mut restored=reopen(&original);
    for editor in [&original,&restored] {
        let scene=editor.document().scene();
        assert_eq!(scene.clipping_base(top),Some(base));assert_eq!(scene.clipping_base(shade),Some(base));
        assert_eq!(scene.attached_effects(shade),[blur,curves]);assert_eq!(scene.effect_owner(curves),Some(shade));
        assert_eq!(scene.occurrence(group).unwrap().blend,LayerBlend::PassThrough);
        assert_eq!(scene.occurrence(group).unwrap().isolated_blend,LayerBlend::Multiply);
    }
    while original.undo().unwrap() {assert!(restored.undo().unwrap());assert_editor(&original,&restored);}
    assert!(!restored.undo().unwrap());let mut restored=reopen(&restored);
    while original.redo().unwrap() {assert!(restored.redo().unwrap());assert_editor(&original,&restored);}
    assert!(!restored.redo().unwrap());
    assert!(original.undo().unwrap());assert!(restored.undo().unwrap());
    for editor in [&mut original,&mut restored] {
        let at=editor.document().scene().children(None).iter().position(|handle|*handle==curves).unwrap();
        editor.perform(editor.document().reparent_occurrence_edit(blur,None,at).unwrap()).unwrap();
        assert!(!editor.can_redo());assert_eq!(editor.document().scene().attached_effects(shade),[curves,blur]);
    }
    let restored=reopen(&restored);assert_editor(&original,&restored);
}

#[test]
fn custom_opaque_color_parameters_and_authored_alpha_survive_restart_and_history() {
    let mut original=fixture();let art=&original.document().artwork;
    let effect=art.effects.iter().find(|(_,_,effect)|art.definitions.get(effect.definition).unwrap().program.id.as_ref()=="color_lookup").unwrap().0;
    let definition=art.effects.get(effect).unwrap().definition;
    let mut program=crate::effect_catalog::custom_program("black_white");
    let mut instance=EffectInstance::new(program.clone());
    instance.set("tint_color",EffectValue::Color(RgbColor::from_linear(RgbSpace::DisplayP3,[0.1,0.2,1.75,0.375]).unwrap())).unwrap();
    assert!(program.parameters.iter().find(|parameter|parameter.key.as_ref()=="tint_color").unwrap().opaque);
    original.perform(Edit::Batch(vec![
        Edit::Definition(RecordChange::replace(&art.definitions,definition,Some(Definition {program:program.clone()})).unwrap()),
        Edit::Effect(RecordChange::replace(&art.effects,effect,Some(EffectApplication {definition,values:instance.values,domain:[19,11]})).unwrap()),
    ])).unwrap();
    Arc::make_mut(&mut Arc::make_mut(&mut program).parameters).iter_mut().find(|parameter|parameter.key.as_ref()=="tint_color").unwrap().opaque=false;
    let mut instance=EffectInstance::new(program.clone());
    instance.set("tint_color",EffectValue::Color(RgbColor::from_linear(RgbSpace::DisplayP3,[0.1,0.2,1.75,0.375]).unwrap())).unwrap();
    original.perform(Edit::Batch(vec![
        Edit::Definition(RecordChange::replace(&original.document().artwork.definitions,definition,Some(Definition {program})).unwrap()),
        Edit::Effect(RecordChange::replace(&original.document().artwork.effects,effect,Some(EffectApplication {definition,values:instance.values,domain:[19,11]})).unwrap()),
    ])).unwrap();
    original.undo().unwrap();let mut restored=reopen(&original);
    assert!(original.undo().unwrap());assert!(restored.undo().unwrap());assert_editor(&original,&restored);
    let mut restored=reopen(&restored);
    while original.redo().unwrap() {assert!(restored.redo().unwrap());assert_editor(&original,&restored);}
    assert!(!restored.redo().unwrap());let restored=reopen(&restored);assert_editor(&original,&restored);
    let art=&restored.document().artwork;let application=art.effects.get(effect).unwrap();let program=&art.definitions.get(definition).unwrap().program;
    assert!(!program.parameters.iter().find(|parameter|parameter.key.as_ref()=="tint_color").unwrap().opaque);
    let Some(EffectValue::Color(color))=crate::EffectView::new(program,&application.values).value("tint_color") else {panic!()};
    assert_eq!(color.rgba[3],0.375);
}
