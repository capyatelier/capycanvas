use crate::GradientStop;
use crate::{authored::{Definition, Dimension, Resource}, effect_catalog::ResourceLabel, effects::*, Lut3d};
use super::values::{self, DecodeError, DecodeResult, object, required, string, boolean, array, finite_f32, u32_value};
use serde_json::{json, Map, Value};
use std::{collections::BTreeSet, sync::Arc};

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
    let values=value.as_array().ok_or("Expected an effect array")?;
    if values.len()>maximum {return Err(unsupported("array length"));}
    Ok(values)
}
fn list<'a>(fields: &'a Map<String, Value>, key: &str, maximum: usize) -> DecodeResult<&'a [Value]> {
    fields.get(key).map_or(Ok(&[]), |v| bounded(v, maximum))
}
fn text(value: &Value) -> DecodeResult<Arc<str>> {
    let text = string(value)?;
    if text.is_empty() { return Err("Invalid effect string".into()); }
    if text.len() > 256 { return Err(unsupported("string length")); }
    Ok(text.into())
}
fn label(value: &Value) -> DecodeResult<ResourceLabel> {
    let result = ResourceLabel::Literal(string(value)?.into());
    if !result.valid(256) { return Err(unsupported("label")); }
    Ok(result)
}
fn encode_label(value: &ResourceLabel) -> Value {
    match value { ResourceLabel::Literal(text) => json!(text), ResourceLabel::Message {..} => unreachable!("Custom labels are validated before encoding") }
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
        EffectValue::Gradient(gradient) => {
            let crate::GradientDefinition {stops,interpolation}=gradient;
            json!({"stops":stops.iter().map(|stop| {
                let GradientStop {position,color}=stop;
                Ok(json!({"position":position,"color":values::encode_rgb_color(*color)?}))
            }).collect::<Result<Vec<_>,String>>()?,"interpolation":values::encode_mix_space(*interpolation)})
        },
        EffectValue::Lut3d(None) => Value::Null, EffectValue::Lut3d(Some(resource)) => writer.lut(resource)?,
    };
    Ok(json!({"kind":kind_name(kind),"value":payload}))
}
fn decode_gradient_stops(value: &Value) -> DecodeResult<Vec<GradientStop>> {
    bounded(value,32)?.iter().map(|v| {
        let fields=object(v,&["position","color"])?;
        Ok(GradientStop {position:finite_f32(required(fields,"position")?)?,color:values::parse_rgb_color(required(fields,"color")?)?})
    }).collect()
}
fn builtin_version(id: &str) -> u32 {
    match id {"gradient_map"|"gradient_fill"|"denoise"|"domain_warp"|"posterize"|"kaleidoscope"=>2,_=>1}
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
        EffectParameterKind::Gradient => {
            let fields=object(value,&["stops","interpolation"])?;
            let gradient=crate::GradientDefinition {
                stops:decode_gradient_stops(required(fields,"stops")?)?,
                interpolation:values::parse_mix_space(required(fields,"interpolation")?)?,
            };
            gradient.validate()?;EffectValue::Gradient(gradient)
        },
        EffectParameterKind::Lut3d => EffectValue::Lut3d(if value.is_null() {None} else {Some(reader.lut(value)?)}),
    })
}
fn encode_kind(kind: &EffectParameterKind) -> Value {
    match kind {
        EffectParameterKind::Number {min,max,unit,..} => {
            let mut fields = json!({"kind":"number","min":min,"max":max});
            if !unit.is_empty() { fields["unit"] = json!(unit); } fields
        },
        EffectParameterKind::Choice {options} => json!({"kind":"choice","options":options.iter().map(|option| match option {
            EffectOption::Literal(value) => json!(value), EffectOption::Labeled {value,label} => json!({"value":value,"label":encode_label(label)})
        }).collect::<Vec<_>>()}),
        EffectParameterKind::Toggle|EffectParameterKind::Color|EffectParameterKind::Curve|EffectParameterKind::Gradient|EffectParameterKind::Lut3d=>json!({"kind":kind_name(kind)}),
    }
}
fn decode_kind(value: &Value) -> DecodeResult<EffectParameterKind> {
    let tag = string(required(value.as_object().ok_or("Expected parameter kind object")?, "kind")?)?;
    Ok(match tag {
        "number" => {
            let fields = object(value,&["kind","min","max","unit"])?;
            EffectParameterKind::Number { min:finite_f32(required(fields,"min")?)?, max:finite_f32(required(fields,"max")?)?,
                step:0.01, decimals:6,
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
    let (dimension,reference) = match dimension {Dimension::Scalar => return, Dimension::Count=>("count",None), Dimension::Angle=>("angle",None), Dimension::Time=>("time",None),
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
    Ok(match tag {"scalar"=>Dimension::Scalar,"count"=>Dimension::Count,"angle"=>Dimension::Angle,"time"=>Dimension::Time,
        "length" => match string(required(fields,"reference")?)? {"source_pixels"=>Dimension::SourcePixels,
            "composition_pixels"=>Dimension::CompositionPixels,"normalized"=>Dimension::Normalized,_=>return Err(unsupported("length reference"))},
        _=>return Err(unsupported("dimension"))})
}

pub fn encode_definition(definition: &Definition, writer: &mut impl ResourceWriter) -> Result<Value,String> {
    let Definition {program}=definition;
    if let Some(builtin)=crate::bundled_effect_catalog().get(&program.id) {
        if program != &builtin.program() { return Err("Reserved built-in filter ID".into()); }
        return Ok(json!({"builtin":program.id,"version":builtin_version(&program.id)}));
    }
    if !program.literal_labels() { return Err("Custom filters require literal labels".into()); }
    let EffectProgram {id,label,kind,alpha,space,wgsl,entry,passes,time,lookups,auxiliary,parameters:program_parameters,constraints,..}=program.as_ref();
    text(&json!(id)).map_err(|error|error.to_string())?;
    EffectInstance::new(program.clone()).validate().map_err(str::to_string)?;
    let mut parameters=Map::new();
    for parameter in program_parameters.iter() {
        let EffectParameter {dimension,opaque,key,label:parameter_label,kind:parameter_kind,default,..}=parameter;
        let mut fields=Map::new();
        fields.insert("kind".into(),encode_kind(parameter_kind));
        fields.insert("default".into(),encode_value(default,parameter_kind,writer)?);
        fields.insert("label".into(),encode_label(parameter_label));
        if *opaque {fields.insert("opaque".into(),json!(true));}
        encode_dimension(&mut fields,dimension);
        parameters.insert(key.to_string(),Value::Object(fields));
    }
    let mut data=json!({"key":id,"contract":"capy.filter/1","label":encode_label(label),
        "kind":match kind {EffectKind::Adjustment=>"adjustment",EffectKind::Generator=>"generator"},
        "code":encode_code(wgsl,writer)?,"entry":entry,"parameters":parameters,
        "slots":program_parameters.iter().map(|p| &p.key).collect::<Vec<_>>()});
    match alpha {EffectAlpha::Preserve=>{},EffectAlpha::Filter=>{data["alpha"]=json!("filter");}}
    match space {EffectSpace::Linear=>{},EffectSpace::Blending=>{data["space"]=json!("blending");}}
    if *time {data["time"]=json!(true);}
    if !passes.is_empty() {data["passes"]=Value::Array(passes.iter().map(|pass| {let EffectPass {entry,sampling}=pass;json!({"entry":entry,"sampling":match sampling {
        EffectSampling::Neighborhood {radius}=>json!({"kind":"neighborhood","radius":radius}),
        EffectSampling::Parameter {key,scale,padding}=>json!({"kind":"parameter","key":key,"scale":scale,"padding":padding}),
        EffectSampling::Document=>json!({"kind":"document"})}})}).collect());}
    if !lookups.is_empty() {data["lookups"]=Value::Array(lookups.iter().map(|lookup| {let EffectLookup {wgsl,entry,dependencies,values,workgroup_size,workgroups}=lookup;Ok(json!({"code":encode_code(wgsl,writer)?,
        "entry":entry,"dependencies":dependencies,"values":values,"workgroup_size":workgroup_size,"workgroups":workgroups}))})
        .collect::<Result<_,String>>()?);}
    if let Some(auxiliary)=auxiliary {data["auxiliary"]=match auxiliary {
        EffectAuxiliary::Lut3d {resource,color_space}=>json!({"kind":"lut3d","resource":resource,"color_space":color_space}),
        EffectAuxiliary::Analysis {analysis}=>json!({"kind":"analysis","analysis":match analysis {EffectAnalysisKind::LocalIllumination=>"local_illumination",EffectAnalysisKind::Dehaze=>"dehaze"}})};}
    if !constraints.is_empty() {data["constraints"]=json!(constraints.iter().map(|constraint|match constraint {
        EffectConstraint::OrderedNumbers {lower,upper,gap}=>json!({"kind":"ordered_numbers","lower":lower,"upper":upper,"gap":gap})}).collect::<Vec<_>>());}
    Ok(data)
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
    if value.get("builtin").is_some() {
        let fields=object(value,&["builtin","version"])?;
        let id=string(required(fields,"builtin")?)?;
        if u32_value(required(fields,"version")?)?!=builtin_version(id) {return Err(unsupported("built-in parameter version"));}
        let builtin=crate::bundled_effect_catalog().get(id).ok_or_else(||unsupported("built-in ID"))?;
        return Ok(Definition {program:builtin.program()});
    }
    let fields=object(value,&["key","contract","label","kind","code","entry","parameters","slots","alpha","space",
        "time","passes","lookups","auxiliary","constraints"])?;
    if string(required(fields,"contract")?)? != "capy.filter/1" {return Err(unsupported("evaluation contract"));}
    if crate::bundled_effect_catalog().get(string(required(fields,"key")?)?).is_some() { return Err("Reserved built-in filter ID".into()); }
    let parameter_records=required(fields,"parameters")?.as_object().ok_or("Expected keyed parameters")?;
    if parameter_records.len()>64 {return Err(unsupported("parameter count"));}
    let slots=bounded(required(fields,"slots")?,64)?.iter().map(text).collect::<DecodeResult<Vec<_>>>()?;
    if slots.len()!=parameter_records.len() || slots.iter().collect::<BTreeSet<_>>().len()!=slots.len()
        || slots.iter().any(|key|!parameter_records.contains_key(key.as_ref())) {return Err("Invalid effect ABI slots".into());}
    let mut parameters=Vec::new();
    for key in slots {
        let record=object(&parameter_records[key.as_ref()],&["kind","default","label","dimension","reference","opaque"])?;
        let kind=decode_kind(required(record,"kind")?)?;
        let default=decode_value(required(record,"default")?,&kind,reader)?;
        parameters.push(EffectParameter {dimension:decode_dimension(record)?,opaque:record.get("opaque").map(boolean).transpose()?.unwrap_or(false),key,label:label(required(record,"label")?)?,
            section:None,page:None,visible_when:None,soft_bounds:None,mapping:NumericMapping::Linear,kind,default});
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
    let constraints=list(fields,"constraints",128)?.iter().map(|v| {
        let fields=v.as_object().ok_or("Expected constraint object")?;
        if string(required(fields,"kind")?)?!="ordered_numbers" {return Err(unsupported("constraint"));}
        object(v,&["kind","lower","upper","gap"])?;
        Ok(EffectConstraint::OrderedNumbers {lower:text(required(fields,"lower")?)?,upper:text(required(fields,"upper")?)?,gap:finite_f32(required(fields,"gap")?)?})
    }).collect::<DecodeResult<Vec<_>>>()?;
    let kind=match string(required(fields,"kind")?)? {"adjustment"=>EffectKind::Adjustment,"generator"=>EffectKind::Generator,_=>return Err(unsupported("kind"))};
    let alpha=match fields.get("alpha").map(string).transpose()?.unwrap_or("preserve") {"preserve"=>EffectAlpha::Preserve,"filter"=>EffectAlpha::Filter,_=>return Err(unsupported("alpha"))};
    let space=match fields.get("space").map(string).transpose()?.unwrap_or("linear") {"linear"=>EffectSpace::Linear,"blending"=>EffectSpace::Blending,_=>return Err(unsupported("space"))};
    let program=Arc::new(EffectProgram {abi:EFFECT_ABI,id:text(required(fields,"key")?)?,label:label(required(fields,"label")?)?,kind,alpha,space,resolution:EffectResolution::Native,
        wgsl:decode_code(required(fields,"code")?,reader)?,entry:text(required(fields,"entry")?)?,passes:passes.into(),
        time:fields.get("time").map(boolean).transpose()?.unwrap_or(false),lookups:lookups.into(),
        constant_color:None,
        auxiliary:fields.get("auxiliary").map(decode_auxiliary).transpose()?,pages:Arc::default(),parameters:parameters.into(),constraints:constraints.into()});
    EffectInstance::new(program.clone()).validate()?;
    Ok(Definition {program})
}

pub fn encode_values(program: &Arc<EffectProgram>, values: &[EffectValue], writer: &mut impl ResourceWriter) -> Result<Value,String> {
    EffectInstance {program:program.clone(),values:values.to_vec()}.validate().map_err(str::to_string)?;
    let mut fields=Map::new();
    for (parameter,value) in program.parameters.iter().zip(values) {
        fields.insert(parameter.key.to_string(),encode_value(value,&parameter.kind,writer)?);
    }
    Ok(Value::Object(fields))
}
pub fn decode_values(program: &Arc<EffectProgram>, value: &Value, reader: &mut impl ResourceReader) -> DecodeResult<Vec<EffectValue>> {
    let fields=value.as_object().ok_or("Expected keyed effect values")?;
    if fields.keys().any(|key|!program.parameters.iter().any(|p|p.key.as_ref()==key)) {return Err(unsupported("parameter key"));}
    if fields.len()!=program.parameters.len() {return Err("Missing authored effect parameter".into());}
    let values=program.parameters.iter().map(|parameter| {
        let value=decode_value(required(fields,&parameter.key)?,&parameter.kind,reader)?;
        if let EffectValue::Number(v)=value && !parameter.accepts(v) {return Err(unsupported("parameter value"));}
        Ok(value)
    }).collect::<DecodeResult<Vec<_>>>()?;
    let instance=EffectInstance {program:program.clone(),values};
    if let Some(EffectAuxiliary::Lut3d {color_space,..})=&program.auxiliary
        && let Some(lut)=instance.view().lut3d()
        && let Some(space)=instance.choice(color_space).and_then(crate::color::RgbSpace::from_id)
        && !lut.accepts(space) {return Err(unsupported("color lookup evaluator range"));}
    instance.validate()?;
    Ok(instance.values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
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
        Definition {program:crate::effect_catalog::custom_program("exposure")}
    }
    #[test]
    fn builtin_data_contracts_are_keyed_and_independent_of_shader_layout() {
        let mut actual=Map::new();
        for filter in crate::bundled_effect_catalog().filters() {
            let program=filter.program();
            let parameters:Map<_,_>=program.parameters.iter().map(|p| {
                let mut kind=if let EffectParameterKind::Choice {options}=&p.kind {
                    let choices:BTreeSet<_>=options.iter().map(EffectOption::value).collect();
                    json!({"kind":"choice","options":choices})
                } else {encode_kind(&p.kind)};
                kind.as_object_mut().unwrap().remove("unit");
                if let EffectParameterKind::Number {min,max,..}=p.kind
                    && matches!(p.dimension,Dimension::SourcePixels|Dimension::CompositionPixels) {
                    kind["min"]=json!(if min<0. {min.min(-MAX_PIXEL_LENGTH)}else{0.});
                    kind["max"]=json!(max.max(MAX_PIXEL_LENGTH));
                }
                (p.key.to_string(),json!({"kind":kind,"dimension":p.dimension,"opaque":p.opaque}))
            }).collect();
            actual.insert(filter.id().into(),json!({"version":builtin_version(filter.id()),"kind":program.kind,
                "alpha":program.alpha,"space":program.space,"time":program.time,"parameters":parameters,"constraints":program.constraints}));
        }
        let expected:Value=serde_json::from_str(include_str!("codec/fixtures/builtin-contracts.json")).unwrap();
        for (id, contract) in expected.as_object().unwrap() {
            let mut supported = actual.get(id).unwrap_or_else(|| panic!("Saved built-in {id} is missing")).clone();
            for (key, parameter) in contract["parameters"].as_object().unwrap() {
                let kind = &mut supported["parameters"][key]["kind"];
                match parameter["kind"]["kind"].as_str() {
                    Some("number") => {
                        assert!(kind["min"].as_f64().unwrap() <= parameter["kind"]["min"].as_f64().unwrap(), "{id}.{key} minimum narrowed");
                        assert!(kind["max"].as_f64().unwrap() >= parameter["kind"]["max"].as_f64().unwrap(), "{id}.{key} maximum narrowed");
                        kind["min"] = parameter["kind"]["min"].clone();
                        kind["max"] = parameter["kind"]["max"].clone();
                    },
                    Some("choice") => {
                        for choice in parameter["kind"]["options"].as_array().unwrap() {
                            assert!(kind["options"].as_array().unwrap().contains(choice), "{id}.{key} lost choice {choice}");
                        }
                        kind["options"] = parameter["kind"]["options"].clone();
                    },
                    _ => {},
                }
            }
            assert_eq!(&supported, contract, "Saved built-in {id} changed its contract");
        }
    }
    #[test]
    fn saved_builtin_choices_keep_shader_meaning_when_controls_move_or_change_labels() {
        for (id,key,slot,choices) in [("curves","domain",261,&["encoded_rgb","log_hdr"][..]),
            ("selective_color","mode",37,&["relative","absolute"][..]),
            ("gradient_fill","style",66,&["linear","radial","reflected"][..])] {
            let original=crate::bundled_effect_catalog().get(id).unwrap().program();
            let mut program=original.clone();
            let parameter=Arc::make_mut(&mut Arc::make_mut(&mut program).parameters).iter_mut().find(|p|p.key.as_ref()==key).unwrap();
            let EffectParameterKind::Choice {options}=&mut parameter.kind else {panic!()};
            Arc::make_mut(options).reverse();
            for option in Arc::make_mut(options) {*option=EffectOption::Labeled {value:option.value().into(),label:"Renamed choice".into()};}
            for (code,choice) in choices.iter().enumerate() {
                let mut saved=encode_values(&original,&EffectInstance::new(original.clone()).values,&mut Resources::default()).unwrap();
                saved[key]=json!({"kind":"choice","value":choice});
                let values=decode_values(&program,&saved,&mut Resources::default()).unwrap();
                let view=EffectView::new(&program,&values);
                assert_eq!(view.choice(key),Some(*choice));
                assert_eq!(view.gpu_parameters(crate::color::RgbSpace::Srgb).unwrap()[slot][0],code as f32);
                let mut inserted=EffectInstance::new(program.clone());
                inserted.set_choice(key,choice).unwrap();
                assert_eq!(inserted.choice(key),Some(*choice));
            }
        }
    }
    #[test]
    fn builtins_save_only_identity_and_all_authored_values() {
        for filter in crate::bundled_effect_catalog().filters() {
            let definition=Definition {program:filter.program()};
            let mut resources=Resources::default();
            let encoded=encode_definition(&definition,&mut resources).unwrap();
            assert_eq!(encoded,json!({"builtin":filter.id(),"version":builtin_version(filter.id())}));
            let decoded=decode_definition(&encoded,&mut resources).unwrap();
            assert!(Arc::ptr_eq(&decoded.program,&definition.program));
            assert!(resources.code.is_empty());
            for instance in [EffectInstance::new(filter.program()),filter.preview().unwrap()] {
                let values=encode_values(&definition.program,&instance.values,&mut resources).unwrap();
                assert_eq!(values.as_object().unwrap().len(),definition.program.parameters.len());
                assert_eq!(decode_values(&decoded.program,&values,&mut resources).unwrap(),instance.values);
                if let Some(parameter)=definition.program.parameters.first() {
                    let mut incomplete=values;incomplete.as_object_mut().unwrap().remove(parameter.key.as_ref());
                    assert!(decode_values(&decoded.program,&incomplete,&mut resources).is_err());
                }
            }
        }
    }
    #[test]
    fn builtin_versions_ids_and_embedded_overrides_are_explicit() {
        let mut resources=Resources::default();
        for value in [json!({"builtin":"exposure","version":2}),json!({"builtin":"gradient_map","version":1}),json!({"builtin":"gradient_fill","version":1}),json!({"builtin":"unknown","version":1})] {
            assert!(matches!(decode_definition(&value,&mut resources),Err(DecodeError::Unsupported(_))));
        }
        let mut definition=fixture();
        Arc::make_mut(&mut definition.program).id="exposure".into();
        assert!(encode_definition(&definition,&mut resources).is_err());
        let mut embedded=encode_definition(&fixture(),&mut resources).unwrap();
        embedded["key"]=json!("exposure");
        assert!(matches!(decode_definition(&embedded,&mut resources),Err(DecodeError::Invalid(_))));
    }
    #[test]
    fn custom_definitions_preserve_code_and_literal_metadata() {
        for filter in crate::bundled_effect_catalog().filters() {
            let definition=Definition {program:crate::effect_catalog::custom_program(filter.id())};
            let mut resources=Resources::default();
            let encoded=encode_definition(&definition,&mut resources).unwrap();
            let decoded=decode_definition(&encoded,&mut resources).unwrap();
            for field in ["abi","pages","resolution","constant_color"] {assert!(encoded.get(field).is_none());}
            for parameter in encoded["parameters"].as_object().unwrap().values() {
                for field in ["section","page","visible_when","soft_bounds","mapping"] {assert!(parameter.get(field).is_none());}
                for field in ["step","decimals"] {assert!(parameter["kind"].get(field).is_none());}
            }
            let original=EffectInstance::new(definition.program.clone());
            let restored=EffectInstance::new(decoded.program.clone());
            assert_eq!(restored.values,original.values);
            assert_eq!(restored.gpu_parameters(crate::color::RgbSpace::Srgb).unwrap(),original.gpu_parameters(crate::color::RgbSpace::Srgb).unwrap());
            assert_eq!(encode_definition(&decoded,&mut resources).unwrap(),encoded);
            for (source,loaded) in definition.program.wgsl.sources().unwrap().iter().zip(decoded.program.wgsl.sources().unwrap()) {
                assert!(source.same_owner(loaded));
            }
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
            (EffectParameterKind::Gradient,EffectValue::Gradient(crate::GradientDefinition {stops:vec![crate::GradientStop {position:0.,color},crate::GradientStop {position:1.,color}],interpolation:crate::ColorMixSpace::Classic})),
            (EffectParameterKind::Lut3d,EffectValue::Lut3d(Some(lut.clone()))),
            (EffectParameterKind::Lut3d,EffectValue::Lut3d(None)),
        ];
        for (kind,value) in cases {
            let encoded=encode_value(&value,&kind,&mut resources).unwrap();
            let decoded=decode_value(&encoded,&kind,&mut resources).unwrap();
            assert_eq!(decoded,value);
            if let EffectValue::Choice(_)=value {assert_eq!(encoded["value"],"second");}
            if let EffectValue::Gradient(_)=value {
                assert_eq!(encoded["value"]["interpolation"],"classic");
                assert!(encoded["value"].get("dither").is_none());
                let mut stale=encoded.clone();stale["value"]=encoded["value"]["stops"].clone();
                assert!(decode_value(&stale,&kind,&mut resources).is_err());
                let mut extra=encoded.clone();extra["value"]["dither"]=json!(false);
                assert!(decode_value(&extra,&kind,&mut resources).is_err());
                let mut future=encoded.clone();future["value"]["interpolation"]=json!("Future");
                assert!(matches!(decode_value(&future,&kind,&mut resources),Err(DecodeError::Unsupported(_))));
                let mut extra=encoded.clone();extra["value"]["stops"][0]["future"]=json!(true);
                assert!(matches!(decode_value(&extra,&kind,&mut resources),Err(DecodeError::Unsupported(_))));
                let mut invalid=encoded.clone();invalid["value"]["stops"][1]["position"]=json!(0.);
                assert!(matches!(decode_value(&invalid,&kind,&mut resources),Err(DecodeError::Invalid(_))));
            }
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
            assert_eq!(decoded.program.parameters[0].dimension,dimension);
            assert_eq!(encode_definition(&decoded,&mut resources).unwrap(),encoded);
            assert_eq!(decoded.program.parameters.iter().map(|p|&p.key).collect::<Vec<_>>(),definition.program.parameters.iter().map(|p|&p.key).collect::<Vec<_>>());
        }
    }
    #[test]
    fn saved_values_ignore_insertion_defaults_control_order_and_slider_bounds() {
        let mut program=crate::bundled_effect_catalog().get("exposure").unwrap().program();
        let parameters=Arc::make_mut(&mut Arc::make_mut(&mut program).parameters);
        parameters[0].default=EffectValue::Number(2.);
        parameters[0].soft_bounds=Some([-1.,1.]);
        parameters.reverse();
        let saved=json!({"exposure":{"kind":"number","value":7.25},"offset":{"kind":"number","value":0},"gamma":{"kind":"number","value":1}});
        let values=decode_values(&program,&saved,&mut Resources::default()).unwrap();
        assert_eq!(EffectView::new(&program,&values).value("exposure"),Some(&EffectValue::Number(7.25)));
        assert_eq!(EffectView::new(&program,&values).value("gamma"),Some(&EffectValue::Number(1.)));
    }
    #[test]
    fn extended_curve_counts_are_unsupported_and_malformed_arrays_are_invalid() {
        let mut resources=Resources::default();
        let points:Vec<_>=(0..33).map(|i|[i as f32/32.;2]).collect();
        let value=json!({"kind":"curve","value":points});
        assert!(matches!(decode_value(&value,&EffectParameterKind::Curve,&mut resources),Err(DecodeError::Unsupported(_))));
        for value in [json!({"kind":"curve","value":{}}),json!({"kind":"curve","value":[[0,0],[1]]})] {
            assert!(matches!(decode_value(&value,&EffectParameterKind::Curve,&mut resources),Err(DecodeError::Invalid(_))));
        }
    }
    #[test]
    fn lookup_choice_ids_survive_option_reordering_and_label_changes() {
        let mut program=crate::bundled_effect_catalog().get("color_lookup").unwrap().program();
        let parameters=Arc::make_mut(&mut Arc::make_mut(&mut program).parameters);
        let EffectParameterKind::Choice {options}=&mut parameters[1].kind else {panic!()};
        Arc::make_mut(options).reverse();
        for option in Arc::make_mut(options) {
            if let EffectOption::Labeled {label,..}=option {*label="Renamed color space".into();}
        }
        let saved=json!({"resource":{"kind":"lut3d","value":null},"color_space":{"kind":"choice","value":"display_p3"},"intensity":{"kind":"number","value":100}});
        let values=decode_values(&program,&saved,&mut Resources::default()).unwrap();
        let view=EffectView::new(&program,&values);
        assert_eq!(view.choice("color_space"),Some("display_p3"));
        assert_eq!(view.gpu_parameters(crate::color::RgbSpace::Srgb).unwrap()[2][0],1.);
    }
}
