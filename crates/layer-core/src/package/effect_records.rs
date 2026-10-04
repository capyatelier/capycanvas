use crate::{authored::{Definition, Dimension, Resource}, effect_catalog::ResourceLabel, effects::*, Lut3d};
use super::values::{self, DecodeError, DecodeResult, object, required, string, boolean, array, finite_f32, u32_value};
use serde_json::{json, Map, Value};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

pub trait ResourceWriter {
    fn code(&mut self, source: &Resource<str>) -> Result<Value, String>;
    fn lut(&mut self, resource: &Lut3d) -> Result<Value, String>;
}
pub trait ResourceReader {
    fn code(&mut self, reference: &Value) -> DecodeResult<Resource<str>>;
    fn lut(&mut self, reference: &Value) -> DecodeResult<Arc<Lut3d>>;
}
fn unsupported(name: &str) -> DecodeError { DecodeError::Unsupported(format!("Unsupported effect {name}")) }
fn bounded(value: &Value, maximum: usize) -> DecodeResult<&[Value]> {
    value.as_array().filter(|v| v.len() <= maximum).map(Vec::as_slice)
        .ok_or_else(|| "Invalid or oversized effect array".into())
}
fn list<'a>(fields: &'a Map<String, Value>, key: &str, maximum: usize) -> DecodeResult<&'a [Value]> {
    fields.get(key).map_or(Ok(&[]), |v| bounded(v, maximum))
}
fn text(value: &Value) -> DecodeResult<Arc<str>> {
    let text = string(value)?;
    if text.is_empty() || text.len() > 256 { return Err("Invalid effect string".into()); }
    Ok(text.into())
}
fn label(value: &Value) -> DecodeResult<ResourceLabel> {
    let result = if let Some(text) = value.as_str() { ResourceLabel::Literal(text.into()) }
        else { ResourceLabel::Message { message: text(required(object(value, &["message"])?, "message")?)? } };
    if !result.valid(256) { return Err("Invalid effect label".into()); }
    Ok(result)
}
fn encode_label(value: &ResourceLabel) -> Value {
    match value { ResourceLabel::Literal(text) => json!(text), ResourceLabel::Message {message} => json!({"message":message}) }
}
fn kind_name(kind: &EffectParameterKind) -> &'static str {
    match kind { EffectParameterKind::Number {..} => "number", EffectParameterKind::Toggle => "toggle",
        EffectParameterKind::Choice {..} => "choice", EffectParameterKind::Color => "color",
        EffectParameterKind::Curve => "curve", EffectParameterKind::Gradient => "gradient", EffectParameterKind::Lut3d => "lut3d" }
}
fn encode_value(value: &EffectValue, kind: &EffectParameterKind, writer: &mut impl ResourceWriter) -> Result<Value, String> {
    let payload = match value {
        EffectValue::Number(v) => json!(v), EffectValue::Toggle(v) => json!(v),
        EffectValue::Choice(index) => {
            let EffectParameterKind::Choice {options} = kind else { return Err("Choice requires options".into()); };
            json!(options.get(*index as usize).ok_or("Invalid effect choice")?.value())
        },
        EffectValue::Color(v) => values::encode_rgb_color(*v)?, EffectValue::Curve(v) => json!(v),
        EffectValue::Gradient(stops) => Value::Array(stops.iter().map(|stop| Ok(json!({"position":stop.position,
            "color":values::encode_rgb_color(stop.color)?}))).collect::<Result<_, String>>()?),
        EffectValue::Lut3d(None) => Value::Null, EffectValue::Lut3d(Some(resource)) => writer.lut(resource)?,
    };
    Ok(json!({"kind":kind_name(kind),"value":payload}))
}
fn decode_value(value: &Value, kind: &EffectParameterKind, reader: &mut impl ResourceReader) -> DecodeResult<EffectValue> {
    let fields = object(value, &["kind", "value"])?;
    let tag = string(required(fields, "kind")?)?;
    if !["number","toggle","choice","color","curve","gradient","lut3d"].contains(&tag) { return Err(unsupported(tag)); }
    if tag != kind_name(kind) { return Err("Effect value kind mismatch".into()); }
    let value = required(fields, "value")?;
    Ok(match kind {
        EffectParameterKind::Number {..} => EffectValue::Number(finite_f32(value)?),
        EffectParameterKind::Toggle => EffectValue::Toggle(boolean(value)?),
        EffectParameterKind::Choice {options} => {
            let choice = string(value)?;
            EffectValue::Choice(options.iter().position(|option| option.value() == choice)
                .ok_or_else(|| unsupported("choice value"))? as u32)
        },
        EffectParameterKind::Color => EffectValue::Color(values::parse_rgb_color(value)?),
        EffectParameterKind::Curve => EffectValue::Curve(bounded(value, 32)?.iter().map(|v| {
            let pair = array(v, 2)?; Ok([finite_f32(&pair[0])?,finite_f32(&pair[1])?])
        }).collect::<DecodeResult<_>>()?),
        EffectParameterKind::Gradient => EffectValue::Gradient(bounded(value, 32)?.iter().map(|v| {
            let fields = object(v, &["position", "color"])?;
            Ok(GradientStop { position: finite_f32(required(fields,"position")?)?, color: values::parse_rgb_color(required(fields,"color")?)? })
        }).collect::<DecodeResult<_>>()?),
        EffectParameterKind::Lut3d => EffectValue::Lut3d(if value.is_null() {None} else {Some(reader.lut(value)?)}),
    })
}
fn encode_kind(kind: &EffectParameterKind) -> Value {
    match kind {
        EffectParameterKind::Number {min,max,step,decimals,unit} => {
            let mut fields = json!({"kind":"number","min":min,"max":max,"step":step,"decimals":decimals});
            if !unit.is_empty() { fields["unit"] = json!(unit); } fields
        },
        EffectParameterKind::Choice {options} => json!({"kind":"choice","options":options.iter().map(|option| match option {
            EffectOption::Literal(value) => json!(value), EffectOption::Labeled {value,label} => json!({"value":value,"label":encode_label(label)})
        }).collect::<Vec<_>>()}),
        _ => json!({"kind":kind_name(kind)}),
    }
}
fn decode_kind(value: &Value) -> DecodeResult<EffectParameterKind> {
    let tag = string(required(value.as_object().ok_or("Expected parameter kind object")?, "kind")?)?;
    Ok(match tag {
        "number" => {
            let fields = object(value,&["kind","min","max","step","decimals","unit"])?;
            let decimals = u32_value(required(fields,"decimals")?)?;
            if decimals > 6 { return Err("Invalid decimal places".into()); }
            EffectParameterKind::Number { min:finite_f32(required(fields,"min")?)?, max:finite_f32(required(fields,"max")?)?,
                step:finite_f32(required(fields,"step")?)?, decimals:decimals as u8,
                unit: fields.get("unit").map(string).transpose()?.unwrap_or("").into() }
        },
        "choice" => {
            let fields = object(value,&["kind","options"])?;
            let options = bounded(required(fields,"options")?,256)?.iter().map(|value| {
                Ok(if value.is_string() { EffectOption::Literal(text(value)?) }
                else { let fields = object(value,&["value","label"])?;
                    EffectOption::Labeled {value:text(required(fields,"value")?)?,label:label(required(fields,"label")?)?} })
            }).collect::<DecodeResult<Vec<_>>>()?;
            if options.is_empty() || options.iter().map(EffectOption::value).collect::<BTreeSet<_>>().len() != options.len() {
                return Err("Empty or repeated choice options".into());
            }
            EffectParameterKind::Choice {options: options.into()}
        },
        "toggle"|"color"|"curve"|"gradient"|"lut3d" => {
            object(value,&["kind"])?;
            match tag {"toggle"=>EffectParameterKind::Toggle,"color"=>EffectParameterKind::Color,"curve"=>EffectParameterKind::Curve,
                "gradient"=>EffectParameterKind::Gradient,_=>EffectParameterKind::Lut3d}
        },
        _ => return Err(unsupported("parameter kind")),
    })
}
fn encode_code(shader: &EffectShader, writer: &mut impl ResourceWriter) -> Result<Value,String> {
    let sources=shader.sources().map_err(str::to_string)?;
    if sources.is_empty() || sources.len()>64 { return Err("Invalid effect code module count".into()); }
    Ok(Value::Array(sources.iter().map(|source| writer.code(source)).collect::<Result<_,_>>()?))
}
fn decode_code(value: &Value, reader: &mut impl ResourceReader) -> DecodeResult<EffectShader> {
    let refs = bounded(value,64)?;
    if refs.is_empty() { return Err("Empty effect code".into()); }
    let sources = refs.iter().map(|reference| reader.code(reference)).collect::<DecodeResult<Vec<_>>>()?;
    Ok(if sources.len()==1 { EffectShader::Code(sources.into_iter().next().unwrap()) }
        else { EffectShader::Linked {sources:sources.into()} })
}
fn encode_dimension(fields: &mut Map<String,Value>, dimension: &Dimension) {
    let (dimension,reference) = match dimension {Dimension::Scalar => return, Dimension::Angle=>("angle",None), Dimension::Time=>("time",None),
        Dimension::SourcePixels=>("length",Some("source_pixels")),Dimension::CompositionPixels=>("length",Some("composition_pixels")),
        Dimension::Normalized=>("length",Some("normalized"))};
    fields.insert("dimension".into(),json!(dimension));
    if let Some(reference)=reference {fields.insert("reference".into(),json!(reference));}
}
fn decode_dimension(fields: &Map<String,Value>) -> DecodeResult<Dimension> {
    let Some(dimension) = fields.get("dimension") else {
        if fields.contains_key("reference") { return Err("Length reference requires dimension".into()); }
        return Ok(Dimension::Scalar);
    };
    let tag=string(dimension)?;
    if tag!="length" && fields.contains_key("reference") {return Err("Only length has a reference".into());}
    Ok(match tag {"scalar"=>Dimension::Scalar,"angle"=>Dimension::Angle,"time"=>Dimension::Time,
        "length" => match string(required(fields,"reference")?)? {"source_pixels"=>Dimension::SourcePixels,
            "composition_pixels"=>Dimension::CompositionPixels,"normalized"=>Dimension::Normalized,_=>return Err(unsupported("length reference"))},
        _=>return Err(unsupported("dimension"))})
}

pub fn encode_definition(definition: &Definition, writer: &mut impl ResourceWriter) -> Result<Value,String> {
    let program=&definition.program;
    text(&json!(program.id)).map_err(|error|error.to_string())?;
    EffectInstance::new(program.clone()).validate().map_err(str::to_string)?;
    let mut parameters=Map::new();
    for parameter in program.parameters.iter() {
        let mut fields=Map::new();
        fields.insert("kind".into(),encode_kind(&parameter.kind));
        fields.insert("default".into(),encode_value(&parameter.default,&parameter.kind,writer)?);
        fields.insert("label".into(),encode_label(&parameter.label));
        if let Some(section)=&parameter.section {fields.insert("section".into(),encode_label(section));}
        if let Some(page)=&parameter.page {fields.insert("page".into(),json!(page));}
        if let Some(condition)=&parameter.visible_when {
            let target=program.parameters.iter().find(|p|p.key==condition.key).ok_or("Unknown visibility parameter")?;
            fields.insert("visible_when".into(),json!({"key":condition.key,"value":encode_value(&condition.value,&target.kind,writer)?}));
        }
        if let Some(bounds)=parameter.soft_bounds {fields.insert("soft_bounds".into(),json!(bounds));}
        match parameter.mapping {NumericMapping::Linear=>{},NumericMapping::Log=>{fields.insert("mapping".into(),json!({"type":"log"}));},
            NumericMapping::Power {exponent}=>{fields.insert("mapping".into(),json!({"type":"power","exponent":exponent}));}}
        encode_dimension(&mut fields,&parameter.dimension);
        parameters.insert(parameter.key.to_string(),Value::Object(fields));
    }
    let mut data=json!({"key":program.id,"contract":"capy.filter/1","abi":program.abi,"label":encode_label(&program.label),
        "kind":match program.kind {EffectKind::Adjustment=>"adjustment",EffectKind::Generator=>"generator"},
        "code":encode_code(&program.wgsl,writer)?,"entry":program.entry,"parameters":parameters,
        "slots":program.parameters.iter().map(|p| &p.key).collect::<Vec<_>>()});
    if program.alpha!=EffectAlpha::Preserve {data["alpha"]=json!("filter");}
    if program.space!=EffectSpace::Linear {data["space"]=json!("blending");}
    if program.resolution!=EffectResolution::Native {data["resolution"]=json!("display");}
    if program.time {data["time"]=json!(true);}
    if let Some(key)=&program.constant_color {data["constant_color"]=json!(key);}
    if !program.passes.is_empty() {data["passes"]=Value::Array(program.passes.iter().map(|pass| json!({"entry":pass.entry,"sampling":match &pass.sampling {
        EffectSampling::Neighborhood {radius}=>json!({"kind":"neighborhood","radius":radius}),
        EffectSampling::Parameter {key,scale,padding}=>json!({"kind":"parameter","key":key,"scale":scale,"padding":padding}),
        EffectSampling::Document=>json!({"kind":"document"})}})).collect());}
    if !program.lookups.is_empty() {data["lookups"]=Value::Array(program.lookups.iter().map(|lookup| Ok(json!({"code":encode_code(&lookup.wgsl,writer)?,
        "entry":lookup.entry,"dependencies":lookup.dependencies,"values":lookup.values,"workgroup_size":lookup.workgroup_size,"workgroups":lookup.workgroups})))
        .collect::<Result<_,String>>()?);}
    if let Some(auxiliary)=&program.auxiliary {data["auxiliary"]=match auxiliary {
        EffectAuxiliary::Lut3d {resource,color_space}=>json!({"kind":"lut3d","resource":resource,"color_space":color_space}),
        EffectAuxiliary::Analysis {analysis}=>json!({"kind":"analysis","analysis":analysis})};}
    if !program.pages.is_empty() {data["pages"]=json!(program.pages.iter().map(|page|json!({"id":page.id,"label":encode_label(&page.label)})).collect::<Vec<_>>());}
    if !program.constraints.is_empty() {data["constraints"]=json!(program.constraints.iter().map(|constraint|match constraint {
        EffectConstraint::OrderedNumbers {lower,upper,gap}=>json!({"kind":"ordered_numbers","lower":lower,"upper":upper,"gap":gap})}).collect::<Vec<_>>());}
    Ok(data)
}
fn decode_mapping(value: &Value) -> DecodeResult<NumericMapping> {
    let fields=value.as_object().ok_or("Expected mapping object")?;
    Ok(match string(required(fields,"type")?)? {
        "linear"=>{object(value,&["type"])?;NumericMapping::Linear},
        "log"=>{object(value,&["type"])?;NumericMapping::Log},
        "power"=>{object(value,&["type","exponent"])?;
            NumericMapping::Power {exponent:required(fields,"exponent")?.as_f64().filter(|v|v.is_finite()).ok_or("Invalid exponent")?}},
        _=>return Err(unsupported("mapping")),
    })
}
fn decode_sampling(value: &Value) -> DecodeResult<EffectSampling> {
    let fields=value.as_object().ok_or("Expected sampling object")?;
    Ok(match string(required(fields,"kind")?)? {
        "neighborhood"=>{object(value,&["kind","radius"])?;EffectSampling::Neighborhood {radius:u32_value(required(fields,"radius")?)?}},
        "parameter"=>{object(value,&["kind","key","scale","padding"])?;EffectSampling::Parameter {
            key:text(required(fields,"key")?)?,scale:finite_f32(required(fields,"scale")?)?,padding:u32_value(required(fields,"padding")?)?}},
        "document"=>{object(value,&["kind"])?;EffectSampling::Document},
        _=>return Err(unsupported("sampling")),
    })
}
fn triple(value: &Value) -> DecodeResult<[u32;3]> {
    let items=array(value,3)?; Ok([u32_value(&items[0])?,u32_value(&items[1])?,u32_value(&items[2])?])
}
fn decode_auxiliary(value: &Value) -> DecodeResult<EffectAuxiliary> {
    let fields=value.as_object().ok_or("Expected auxiliary object")?;
    Ok(match string(required(fields,"kind")?)? {
        "lut3d"=>{object(value,&["kind","resource","color_space"])?;EffectAuxiliary::Lut3d {
            resource:text(required(fields,"resource")?)?,color_space:text(required(fields,"color_space")?)?}},
        "analysis"=>{object(value,&["kind","analysis"])?;
            match string(required(fields,"analysis")?)? {"local_illumination"=>EffectAuxiliary::Analysis {analysis:EffectAnalysisKind::LocalIllumination},
                "dehaze"=>EffectAuxiliary::Analysis {analysis:EffectAnalysisKind::Dehaze},
                _=>return Err(unsupported("analysis"))}},
        _=>return Err(unsupported("auxiliary")),
    })
}

pub fn decode_definition(value: &Value, reader: &mut impl ResourceReader) -> DecodeResult<Definition> {
    let fields=object(value,&["key","contract","abi","label","kind","constant_color","code","entry","parameters","slots","alpha","space",
        "resolution","time","passes","lookups","auxiliary","pages","constraints"])?;
    if string(required(fields,"contract")?)? != "capy.filter/1" {return Err(unsupported("evaluation contract"));}
    let abi=u32_value(required(fields,"abi")?)?;
    if abi!=EFFECT_ABI {return Err(unsupported("shader ABI"));}
    let parameter_records=required(fields,"parameters")?.as_object().ok_or("Expected keyed parameters")?;
    if parameter_records.len()>64 {return Err("Too many effect parameters".into());}
    let slots=bounded(required(fields,"slots")?,64)?.iter().map(text).collect::<DecodeResult<Vec<_>>>()?;
    if slots.len()!=parameter_records.len() || slots.iter().collect::<BTreeSet<_>>().len()!=slots.len()
        || slots.iter().any(|key|!parameter_records.contains_key(key.as_ref())) {return Err("Invalid effect ABI slots".into());}
    let mut kinds=BTreeMap::new();
    for key in &slots {
        let record=object(&parameter_records[key.as_ref()],&["kind","default","label","section","page","visible_when","soft_bounds","mapping","dimension","reference"])?;
        kinds.insert(key.clone(),decode_kind(required(record,"kind")?)?);
    }
    let mut parameters=Vec::new();
    for key in slots {
        let record=parameter_records[key.as_ref()].as_object().unwrap();
        let kind=kinds[&key].clone();
        let default=decode_value(required(record,"default")?,&kind,reader)?;
        let visible_when=record.get("visible_when").map(|value| {
            let fields=object(value,&["key","value"])?;
            let key=text(required(fields,"key")?)?;
            let kind=kinds.get(&key).ok_or("Unknown visibility parameter")?;
            Ok::<_,DecodeError>(EffectVisibility {key,value:decode_value(required(fields,"value")?,kind,reader)?})
        }).transpose()?;
        let soft_bounds=record.get("soft_bounds").map(|v| {
            let pair=array(v,2)?;
            let number=|v:&Value| v.as_f64().filter(|v|v.is_finite()).ok_or_else(||DecodeError::Invalid("Invalid soft bound".into()));
            Ok::<_,DecodeError>([number(&pair[0])?,number(&pair[1])?])
        }).transpose()?;
        parameters.push(EffectParameter {dimension:decode_dimension(record)?,key,label:label(required(record,"label")?)?,
            section:record.get("section").map(label).transpose()?,page:record.get("page").map(text).transpose()?,visible_when,soft_bounds,
            mapping:record.get("mapping").map(decode_mapping).transpose()?.unwrap_or_default(),kind,default});
    }
    let passes=list(fields,"passes",8)?.iter().map(|v| {
        let fields=object(v,&["entry","sampling"])?;
        Ok(EffectPass {entry:text(required(fields,"entry")?)?,sampling:decode_sampling(required(fields,"sampling")?)?})
    }).collect::<DecodeResult<Vec<_>>>()?;
    let lookups=list(fields,"lookups",8)?.iter().map(|v| {
        let fields=object(v,&["code","entry","dependencies","values","workgroup_size","workgroups"])?;
        Ok(EffectLookup {wgsl:decode_code(required(fields,"code")?,reader)?,entry:text(required(fields,"entry")?)?,
            dependencies:bounded(required(fields,"dependencies")?,64)?.iter().map(text).collect::<DecodeResult<Vec<_>>>()?.into(),
            values:u32_value(required(fields,"values")?)?,workgroup_size:triple(required(fields,"workgroup_size")?)?,
            workgroups:triple(required(fields,"workgroups")?)?})
    }).collect::<DecodeResult<Vec<_>>>()?;
    let pages=list(fields,"pages",16)?.iter().map(|v| {
        let fields=object(v,&["id","label"])?;
        Ok(EffectPage {id:text(required(fields,"id")?)?,label:label(required(fields,"label")?)?})
    }).collect::<DecodeResult<Vec<_>>>()?;
    let constraints=list(fields,"constraints",128)?.iter().map(|v| {
        let fields=v.as_object().ok_or("Expected constraint object")?;
        if string(required(fields,"kind")?)?!="ordered_numbers" {return Err(unsupported("constraint"));}
        object(v,&["kind","lower","upper","gap"])?;
        Ok(EffectConstraint::OrderedNumbers {lower:text(required(fields,"lower")?)?,upper:text(required(fields,"upper")?)?,gap:finite_f32(required(fields,"gap")?)?})
    }).collect::<DecodeResult<Vec<_>>>()?;
    let kind=match string(required(fields,"kind")?)? {"adjustment"=>EffectKind::Adjustment,"generator"=>EffectKind::Generator,_=>return Err(unsupported("kind"))};
    let alpha=match fields.get("alpha").map(string).transpose()?.unwrap_or("preserve") {"preserve"=>EffectAlpha::Preserve,"filter"=>EffectAlpha::Filter,_=>return Err(unsupported("alpha"))};
    let space=match fields.get("space").map(string).transpose()?.unwrap_or("linear") {"linear"=>EffectSpace::Linear,"blending"=>EffectSpace::Blending,_=>return Err(unsupported("space"))};
    let resolution=match fields.get("resolution").map(string).transpose()?.unwrap_or("native") {"native"=>EffectResolution::Native,"display"=>EffectResolution::Display,_=>return Err(unsupported("resolution"))};
    let program=Arc::new(EffectProgram {abi,id:text(required(fields,"key")?)?,label:label(required(fields,"label")?)?,kind,alpha,space,resolution,
        wgsl:decode_code(required(fields,"code")?,reader)?,entry:text(required(fields,"entry")?)?,passes:passes.into(),
        time:fields.get("time").map(boolean).transpose()?.unwrap_or(false),lookups:lookups.into(),
        constant_color:fields.get("constant_color").map(text).transpose()?,
        auxiliary:fields.get("auxiliary").map(decode_auxiliary).transpose()?,pages:pages.into(),parameters:parameters.into(),constraints:constraints.into()});
    EffectInstance::new(program.clone()).validate()?;
    Ok(Definition {program})
}

pub fn encode_values(program: &Arc<EffectProgram>, values: &[EffectValue], writer: &mut impl ResourceWriter) -> Result<Value,String> {
    EffectInstance {program:program.clone(),values:values.to_vec()}.validate().map_err(str::to_string)?;
    let mut fields=Map::new();
    for (parameter,value) in program.parameters.iter().zip(values) {
        if value!=&parameter.default {fields.insert(parameter.key.to_string(),encode_value(value,&parameter.kind,writer)?);}
    }
    Ok(Value::Object(fields))
}
pub fn decode_values(program: &Arc<EffectProgram>, value: &Value, reader: &mut impl ResourceReader) -> DecodeResult<Vec<EffectValue>> {
    let fields=value.as_object().ok_or("Expected keyed effect values")?;
    if fields.keys().any(|key|!program.parameters.iter().any(|p|p.key.as_ref()==key)) {return Err(unsupported("parameter key"));}
    let mut instance=EffectInstance::new(program.clone());
    for (parameter,value) in program.parameters.iter().zip(&mut instance.values) {
        if let Some(record)=fields.get(parameter.key.as_ref()) {*value=decode_value(record,&parameter.kind,reader)?;}
    }
    instance.validate()?;
    Ok(instance.values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Resources { code: BTreeMap<String,Resource<str>>, luts: BTreeMap<String,Arc<Lut3d>> }
    impl ResourceWriter for Resources {
        fn code(&mut self, source: &Resource<str>) -> Result<Value,String> {
            let id=source.id().to_string(); self.code.insert(id.clone(),source.clone()); Ok(json!({"ref":id}))
        }
        fn lut(&mut self, resource: &Lut3d) -> Result<Value,String> {
            let id=resource.resource().ok_or("Missing LUT storage")?.id().to_string();
            self.luts.entry(id.clone()).or_insert_with(||Arc::new(resource.clone())); Ok(json!({"ref":id}))
        }
    }
    impl ResourceReader for Resources {
        fn code(&mut self, reference: &Value) -> DecodeResult<Resource<str>> {
            let id=string(required(object(reference,&["ref"])?,"ref")?)?;
            self.code.get(id).cloned().ok_or_else(||"Missing code resource".into())
        }
        fn lut(&mut self, reference: &Value) -> DecodeResult<Arc<Lut3d>> {
            let id=string(required(object(reference,&["ref"])?,"ref")?)?;
            self.luts.get(id).cloned().ok_or_else(||"Missing LUT resource".into())
        }
    }
    fn fixture() -> Definition {
        Definition {program:crate::bundled_effect_catalog().filters()[0].program()}
    }
    #[test]
    fn bundled_definitions_preserve_semantics_slots_modules_and_resource_owners() {
        for filter in crate::bundled_effect_catalog().filters() {
            let definition=Definition {program:filter.program()};
            let mut resources=Resources::default();
            let encoded=encode_definition(&definition,&mut resources).unwrap();
            let decoded=decode_definition(&encoded,&mut resources).unwrap();
            assert_eq!(encode_definition(&decoded,&mut resources).unwrap(),encoded,"{}",filter.id());
            assert_eq!(decoded.program.parameters,definition.program.parameters);
            assert_eq!(decoded.program.constant_color,definition.program.constant_color);
            for (source,reopened) in definition.program.wgsl.sources().unwrap().iter().zip(decoded.program.wgsl.sources().unwrap()) {
                assert_eq!(source.id(),reopened.id()); assert!(source.same_owner(reopened));
                assert_eq!(source.as_ref(),reopened.as_ref());
            }
            for (lookup,reopened) in definition.program.lookups.iter().zip(decoded.program.lookups.iter()) {
                assert_eq!(lookup.wgsl.sources().unwrap().len(),reopened.wgsl.sources().unwrap().len());
                for (source,reopened) in lookup.wgsl.sources().unwrap().iter().zip(reopened.wgsl.sources().unwrap()) {
                    assert!(source.same_owner(reopened));
                }
            }
            let defaults=EffectInstance::new(definition.program.clone()).values;
            assert_eq!(encode_values(&definition.program,&defaults,&mut resources).unwrap(),json!({}));
            assert_eq!(decode_values(&decoded.program,&json!({}),&mut resources).unwrap(),defaults);
            let preview=filter.preview().unwrap();
            let values=encode_values(&definition.program,&preview.values,&mut resources).unwrap();
            assert_eq!(decode_values(&decoded.program,&values,&mut resources).unwrap(),preview.values);
        }
    }
    #[test]
    fn value_grammar_uses_stable_choices_exact_colors_curves_gradients_and_lut_references() {
        let mut resources=Resources::default();
        let color=crate::color::RgbColor::from_linear(crate::color::RgbSpace::DisplayP3,[0.1,0.2,0.3,0.2]).unwrap();
        let lut=Arc::new(Lut3d::from_samples(2,[[0.;3],[1.;3]],"Wire table".into(),
            (0..8).map(|i|[(i&1) as f32,((i>>1)&1) as f32,((i>>2)&1) as f32]).collect::<Vec<_>>().into()).unwrap());
        let cases=[
            (EffectParameterKind::Number {min:0.,max:1.,step:0.1,decimals:2,unit:"".into()},EffectValue::Number(0.75)),
            (EffectParameterKind::Toggle,EffectValue::Toggle(true)),
            (EffectParameterKind::Choice {options:vec![EffectOption::Literal("first".into()),EffectOption::Literal("second".into())].into()},EffectValue::Choice(1)),
            (EffectParameterKind::Color,EffectValue::Color(color)),
            (EffectParameterKind::Curve,EffectValue::Curve(vec![[0.,0.],[0.3,0.7],[1.,1.]])),
            (EffectParameterKind::Gradient,EffectValue::Gradient(vec![GradientStop {position:0.,color},GradientStop {position:1.,color}])),
            (EffectParameterKind::Lut3d,EffectValue::Lut3d(Some(lut.clone()))),
            (EffectParameterKind::Lut3d,EffectValue::Lut3d(None)),
        ];
        for (kind,value) in cases {
            let encoded=encode_value(&value,&kind,&mut resources).unwrap();
            let decoded=decode_value(&encoded,&kind,&mut resources).unwrap();
            assert_eq!(decoded,value);
            if let EffectValue::Choice(_)=value {assert_eq!(encoded["value"],"second");}
            if let EffectValue::Lut3d(Some(reopened))=decoded {
                assert_eq!(encoded["value"],json!({"ref":lut.resource().unwrap().id().to_string()}));
                assert!(Arc::ptr_eq(reopened.storage().unwrap(),lut.storage().unwrap()));
                assert_eq!(reopened.payload(),lut.payload());
            }
        }
    }
    #[test]
    fn unknown_semantics_preserve_and_malformed_known_fields_reject() {
        let mut resources=Resources::default();
        let definition=fixture(); let encoded=encode_definition(&definition,&mut resources).unwrap();
        for (field,value) in [("abi",json!(EFFECT_ABI+1)),("contract",json!("future.filter/1")),("kind",json!("future")),("space",json!("future"))] {
            let mut future=encoded.clone();future[field]=value;
            assert!(matches!(decode_definition(&future,&mut resources),Err(DecodeError::Unsupported(_))));
        }
        let key=definition.program.parameters.first().unwrap().key.as_ref();
        for (field,value) in [("dimension",json!("future")),("future_field",json!(true)),("mapping",json!({"type":"future"}))] {
            let mut future=encoded.clone();future["parameters"][key][field]=value;
            assert!(matches!(decode_definition(&future,&mut resources),Err(DecodeError::Unsupported(_))));
        }
        let mut malformed=encoded.clone();malformed["slots"]=json!([key,key]);
        assert!(matches!(decode_definition(&malformed,&mut resources),Err(DecodeError::Invalid(_))));
        let mut malformed=encoded.clone();malformed["parameters"][key]["dimension"]=json!("length");
        assert!(matches!(decode_definition(&malformed,&mut resources),Err(DecodeError::Invalid(_))));
        let mut malformed=encoded;malformed.as_object_mut().unwrap().remove("entry");
        assert!(matches!(decode_definition(&malformed,&mut resources),Err(DecodeError::Invalid(_))));
        assert!(matches!(decode_values(&definition.program,&json!({"future": {"kind":"number","value":1}}),&mut resources),Err(DecodeError::Unsupported(_))));
        let kind=EffectParameterKind::Choice {options:vec![EffectOption::Literal("known".into())].into()};
        assert!(matches!(decode_value(&json!({"kind":"choice","value":"future"}),&kind,&mut resources),Err(DecodeError::Unsupported(_))));
        assert!(matches!(decode_value(&json!({"kind":"choice","value":0}),&kind,&mut resources),Err(DecodeError::Invalid(_))));
    }
    #[test]
    fn dimensions_are_semantic_and_parameter_map_order_does_not_change_abi() {
        let mut definition=fixture(); let mut resources=Resources::default();
        for dimension in [Dimension::Scalar,Dimension::Angle,Dimension::Time,Dimension::SourcePixels,Dimension::CompositionPixels,Dimension::Normalized] {
            Arc::make_mut(&mut Arc::make_mut(&mut definition.program).parameters)[0].dimension=dimension;
            let encoded=encode_definition(&definition,&mut resources).unwrap();
            let decoded=decode_definition(&encoded,&mut resources).unwrap();
            assert_eq!(decoded.program.parameters,definition.program.parameters);
            assert_eq!(decoded.program.parameters.iter().map(|p|&p.key).collect::<Vec<_>>(),definition.program.parameters.iter().map(|p|&p.key).collect::<Vec<_>>());
        }
    }
}
