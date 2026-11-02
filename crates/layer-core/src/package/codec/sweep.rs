use super::*;
use crate::{ColorMixSpace, EffectParameterKind, TextureSource, color::{RgbColor, ProofRecipe, RenderingIntent, hdr::SdrRendition}, package::{effect_records, registry, values as wire, vector_records}};
use std::collections::{BTreeMap, BTreeSet};

fn color(default: RgbColor) -> RgbColor {
    let first = RgbColor::new(RgbSpace::Srgb, [0.25, 0.5, 0.75, 1.]).unwrap();
    if first != default { first } else { RgbColor::new(RgbSpace::Srgb, [0.75, 0.5, 0.25, 1.]).unwrap() }
}

fn nondefault(mut instance: EffectInstance) -> EffectInstance {
    for parameter in instance.program.clone().parameters.iter() {
        let candidates = match (&parameter.kind, &parameter.default) {
            (EffectParameterKind::Number {min, max, step, ..}, _) => {
                let [low, high] = parameter.soft_bounds.unwrap_or([f64::from(*min), f64::from(*max)]);
                [low + (high-low)*0.25, low + (high-low)*0.75, low, high].into_iter().map(|value| {
                    let value = if *step > 0. { f64::from(*min) + ((value-f64::from(*min))/f64::from(*step)).round()*f64::from(*step) } else { value };
                    EffectValue::Number(value as f32)
                }).collect::<Vec<_>>()
            },
            (EffectParameterKind::Toggle, EffectValue::Toggle(value)) => vec![EffectValue::Toggle(!value)],
            (EffectParameterKind::Choice {options}, EffectValue::Choice(value)) => (0..options.len()).filter(|index| *index != *value as usize).map(|index| EffectValue::Choice(index as u32)).collect(),
            (EffectParameterKind::Color, EffectValue::Color(value)) => vec![EffectValue::Color(color(*value))],
            (EffectParameterKind::Curve, EffectValue::Curve(points)) => [[0.25, 0.375], [0.5, 0.625], [0.75, 0.875]].into_iter()
                .map(|point| EffectValue::Curve(vec![points[0], point, *points.last().unwrap()])).collect(),
            (EffectParameterKind::Gradient, EffectValue::Gradient(value)) => {
                let mut gradient = value.clone();
                gradient.stops[0].color = color(gradient.stops[0].color);
                gradient.interpolation = ColorMixSpace::ALL.into_iter().find(|space| *space != value.interpolation).unwrap();
                vec![EffectValue::Gradient(gradient)]
            },
            (EffectParameterKind::Lut3d, _) => vec![EffectValue::Lut3d(Some(Arc::new(Lut3d::from_samples(2, [[0.;3],[1.;3]], "Exact table".into(), vec![[0.125,0.75,0.5];8].into()).unwrap())))],
            (EffectParameterKind::Texture, _) => vec![EffectValue::Texture(TextureSource::Program(nondefault(EffectInstance::new(crate::bundled_texture_catalog().filters()[0].program()))))],
            _ => panic!("unsupported parameter {}.{}", instance.program.id, parameter.key),
        };
        assert!(candidates.into_iter().any(|value| instance.set(&parameter.key, value).is_ok() && instance.value(&parameter.key) != Some(&parameter.default)),
            "no non-default candidate for {}.{}", instance.program.id, parameter.key);
    }
    instance
}

fn occurrence(artwork: &mut Artwork, content: OccurrenceContent, name: &str) {
    let handle = artwork.occurrences.insert(PortableId::random(), Occurrence::new(content, name)).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(handle);
}

fn effect(artwork: &mut Artwork, instance: EffectInstance, name: &str) {
    let handle = artwork.effects.insert(PortableId::random(), EffectApplication::new(instance.program, instance.values, [64;2])).unwrap();
    occurrence(artwork, OccurrenceContent::Effect(handle), name);
}

fn image(channels: SourceChannels, depth: SampleDepth, profile: ColorProfile, assumed: bool) -> Image {
    let interpretation = SourceInterpretation { channels, depth, profile, profile_assumed: assumed };
    let bytes = vec![0; interpretation.descriptor().byte_len([TILE_SIZE;2]).unwrap()];
    let tile = Arc::new(TileBlob::encode(interpretation.descriptor(), &bytes).unwrap());
    Image::new(Arc::new(SourceImage {extent:[64;2], resolution:Some(crate::ImageResolution::ppi(300)), interpretation, tiles:[([0,0],tile)].into()}))
}

fn save(directory: &Path, name: &str, artwork: &Artwork) {
    std::fs::write(directory.join(name), serialize(&prepare(artwork, true))).unwrap();
}

pub(super) fn generate(directory: &Path) {
    let mut filters = Artwork::new([64;2]).unwrap();
    let mut definitions: Vec<_> = crate::bundled_effect_catalog().filters().iter().collect();
    definitions.sort_by_key(|definition| definition.id());
    let instances: Vec<_> = definitions.iter().map(|definition| nondefault(EffectInstance::new(definition.program()))).collect();
    for instance in &instances { effect(&mut filters, instance.clone(), &instance.program.id); }
    for instance in instances {
        for (parameter, chosen) in instance.program.parameters.iter().zip(&instance.values) {
            let alternatives = match (&parameter.kind, chosen) {
                (EffectParameterKind::Choice {options}, EffectValue::Choice(chosen)) => (0..options.len()).filter(|index| *index != *chosen as usize).map(|index| EffectValue::Choice(index as u32)).collect::<Vec<_>>(),
                (EffectParameterKind::Gradient, EffectValue::Gradient(chosen)) => ColorMixSpace::ALL.into_iter().filter(|space| *space != chosen.interpolation).map(|space| {
                    let EffectValue::Gradient(mut gradient) = parameter.default.clone() else { unreachable!() };
                    gradient.interpolation = space; EffectValue::Gradient(gradient)
                }).collect(),
                _ => Vec::new(),
            };
            for value in alternatives {
                let mut variant = EffectInstance::new(instance.program.clone());
                variant.set(&parameter.key, value).unwrap();
                effect(&mut filters, variant, &instance.program.id);
            }
        }
    }
    for texture in crate::bundled_texture_catalog().filters().iter().skip(1) {
        let mut paper = EffectInstance::new(crate::bundled_effect_catalog().get(crate::paper::PAPER_PROGRAM).unwrap().program());
        paper.set("texture", EffectValue::Texture(TextureSource::Program(nondefault(EffectInstance::new(texture.program()))))).unwrap();
        effect(&mut filters, paper, texture.id());
    }
    let mut paper = EffectInstance::new(crate::bundled_effect_catalog().get(crate::paper::PAPER_PROGRAM).unwrap().program());
    paper.set("texture", EffectValue::Texture(TextureSource::Image(image(SourceChannels::Gray, SampleDepth::U8, ColorProfile::Builtin(RgbSpace::Srgb), false)))).unwrap();
    effect(&mut filters, paper, "image texture");
    save(directory, "filters.capy", &filters);

    let mut layers = Artwork::new([64;2]).unwrap();
    for (channels, depth, profile) in [
        (SourceChannels::GrayAlpha, SampleDepth::U16, ColorProfile::Builtin(RgbSpace::AdobeRgb)),
        (SourceChannels::Rgb, SampleDepth::F16, ColorProfile::Builtin(RgbSpace::ProPhoto)),
        (SourceChannels::Cmyk, SampleDepth::U8, ColorProfile::Icc(Resource::from((0..256).map(|n| n as u8).collect::<Vec<_>>()))),
    ] {
        let object = ImageObject::new(image(channels, depth, profile, channels == SourceChannels::GrayAlpha));
        let object = layers.objects.insert(PortableId::random(), object).unwrap();
        occurrence(&mut layers, OccurrenceContent::Objects(object), "Source channels");
    }
    {
        use crate::authored::vector::*;
        let shape = LiveShape {kind:ShapeKind::Ellipse, center:[16.;2], size:[12.,8.], rotation:0.125};
        let mut stroke = Stroke::new(Paint::color(color(RgbColor::new(RgbSpace::Srgb, [0.,0.,0.,1.]).unwrap())), 2.);
        stroke.miter_limit = 6.5;
        let mut path = PathItem::new(ItemId(1), Arc::new(shape.geometry().unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)])));
        path.shape = Some(Arc::new(shape));
        let vector = layers.vectors.insert(PortableId::random(), VectorObject::from_records(vec![Record::Path(path)])).unwrap();
        occurrence(&mut layers, OccurrenceContent::Vector(vector), "Live shape and miter limit");
    }
    let selection = layers.selections.insert(PortableId::random(), SavedSelection {selection:Selection::pixels(Arc::new(SelectionPixels::new([8,1], [0,0,8,1], vec![0x43214321]).unwrap()))}).unwrap();
    occurrence(&mut layers, OccurrenceContent::Selection(selection), "Packed coverage");
    save(directory, "layers.capy", &layers);

    for (n, depth) in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32].into_iter().enumerate() {
        let mut artwork = Artwork::new([16;2]).unwrap();
        if n == 0 { artwork.metadata = Arc::new(PhotoMetadata {xmp:Some(Resource::from(format!("<xmp>{}</xmp>", "sweep".repeat(32)).into_bytes())), ..Default::default()}); }
        let composition = artwork.compositions.get_mut(artwork.root).unwrap();
        composition.color = DocumentColor {space:RgbSpace::ALL[n], depth};
        composition.blend = crate::BlendSpace::ALL[n % 2].for_depth(depth);
        composition.resolution = Some(crate::ImageResolution {unit:[crate::ResolutionUnit::Inch,crate::ResolutionUnit::Centimetre,crate::ResolutionUnit::Metre][n % 3], density:[[300,1];2]});
        let output = artwork.outputs.get_mut(artwork.default_output).unwrap();
        let mut proof = ProofRecipe::new("Sweep proof".into(), ColorProfile::Builtin(RgbSpace::ALL[n]));
        if n == 0 { proof.profile = ColorProfile::Icc(Resource::from((0..256).map(|n| n as u8).collect::<Vec<_>>().repeat(4))); }
        proof.conversion.intent = [RenderingIntent::Perceptual, RenderingIntent::RelativeColorimetric, RenderingIntent::Saturation, RenderingIntent::AbsoluteColorimetric][n];
        proof.conversion.black_point_compensation = n % 2 == 0;
        proof.simulate_paper = n % 2 != 0;
        output.proof = Some(proof);
        output.sdr = SdrRendition {exposure:0.5, contrast:1.25, headroom:3., highlight_color:0.5, balance:0.25};
        save(directory, &format!("document-{n}.capy"), &artwork);
    }
}

fn collect(value: &Value, records: &mut Vec<Value>) {
    match value {
        Value::Object(fields) => {
            if fields.get("type").and_then(Value::as_str).is_some_and(|kind| registry::descriptor(kind).is_some()) { records.push(value.clone()); }
            for value in fields.values() { collect(value, records); }
        },
        Value::Array(values) => for value in values { collect(value, records); },
        _ => {},
    }
}

fn inputs(directory: &Path, records: &mut Vec<Value>, kinds: &mut BTreeSet<u64>, tags: &mut BTreeSet<u64>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() { inputs(&path, records, kinds, tags); }
        else if path.extension().is_some_and(|extension| extension == "capy") {
            if path.file_name().unwrap() == "unsupported-pass-through.capy" { continue; }
            let bytes = std::fs::read(&path).unwrap();
            let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
            let value: Value = serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
            collect(&value, records);
            let artwork = editable(bytes);
            for resource in &prepare(&artwork, false).resources().entries {
                if resource.record["type"] != "capy.vector-items/1" { continue; }
                let encoded = resource.payload.encoded().unwrap();
                let raw = match encoded.encoding {
                    ResourceEncoding::Raw => encoded.bytes.to_vec(),
                    ResourceEncoding::Lz4 => lz4_flex::block::decompress(&encoded.bytes, resource.record["data"]["decoded_bytes"].as_str().unwrap().parse().unwrap()).unwrap(),
                };
                let (found_kinds, found_tags) = vector_records::item_words(&raw).unwrap();
                kinds.extend(found_kinds); tags.extend(found_tags);
            }
        } else if path.parent().unwrap().file_name().is_some_and(|name| name == "generations") {
            collect(&serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap(), records);
        }
    }
}

pub(super) fn check(directory: &Path) {
    let (mut records, mut kinds, mut tags) = (Vec::new(), BTreeSet::new(), BTreeSet::new());
    inputs(&directory.join("codec"), &mut records, &mut kinds, &mut tags);
    let mut private = Vec::new();
    inputs(&directory.join("sessions"), &mut private, &mut BTreeSet::new(), &mut BTreeSet::new());
    let mut missing = Vec::new();
    for descriptor in registry::RECORDS {
        let source = if descriptor.type_id == "capy.wgsl/1" { &private } else { &records };
        let values: Vec<_> = source.iter().filter(|record| record["type"] == descriptor.type_id).map(|record| &record["data"]).collect();
        assert!(!values.is_empty(), "missing record kind: {}", descriptor.type_id);
        for field in descriptor.data_fields {
            if matches!((descriptor.type_id, *field), ("capy.coverage-source/2", "material") | ("capy.lut3d/1", "title")) { continue; }
            let present = if (descriptor.type_id, *field) == ("capy.effect/2", "program") {
                private.iter().any(|record| record["type"] == descriptor.type_id && record["data"].get(*field).is_some())
            } else { values.iter().any(|value| value.get(*field).is_some()) };
            if !present { missing.push(format!("{}.{field}", descriptor.type_id)); }
        }
    }
    assert!(missing.is_empty(), "missing record fields: {missing:?}");
    for (catalog, texture) in [(crate::bundled_effect_catalog(), false), (crate::bundled_texture_catalog(), true)] {
        for definition in catalog.filters() {
            let program = definition.program();
            let mut applications = Vec::new();
            fn find<'a>(value: &'a Value, id: &str, texture: bool, applications: &mut Vec<&'a Value>) {
                let application = if texture && value["kind"] == "texture" { Some(&value["value"]) }
                    else if !texture && value["type"] == "capy.effect/2" { Some(&value["data"]) } else { None };
                if let Some(application) = application
                    && application.get("builtin").and_then(Value::as_str) == Some(id)
                    && let Some(values) = application.get("values") { applications.push(values); }
                match value { Value::Object(values) => for value in values.values() { find(value, id, texture, applications); }, Value::Array(values) => for value in values { find(value, id, texture, applications); }, _ => {} }
            }
            for record in &records { find(record, definition.id(), texture, &mut applications); }
            assert!(!applications.is_empty(), "missing builtin: {}", definition.id());
            let defaults = EffectInstance::new(program.clone());
            let defaults = effect_records::encode_values(&program, &defaults.values, &mut resources::ResourceInventory::default()).unwrap();
            for parameter in program.parameters.iter() {
                assert!(applications.iter().any(|values| values.get(parameter.key.as_ref()).is_some_and(|value| value != &defaults[parameter.key.as_ref()])), "missing non-default parameter: {}.{}", definition.id(), parameter.key);
                if let EffectParameterKind::Choice {options} = &parameter.kind {
                    for option in options.iter() {
                        assert!(applications.iter().any(|values| values[parameter.key.as_ref()]["value"] == option.value()), "missing choice: {}.{}={}", definition.id(), parameter.key, option.value());
                    }
                }
                if parameter.kind == EffectParameterKind::Gradient {
                    for space in ColorMixSpace::ALL {
                        assert!(applications.iter().any(|values| values[parameter.key.as_ref()]["value"]["interpolation"] == wire::encode_mix_space(space)), "missing gradient interpolation: {}.{}={space:?}", definition.id(), parameter.key);
                    }
                }
            }
        }
    }
    let mut seen: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut add = |label, value: Value| { seen.entry(label).or_default().insert(value.to_string()); };
    fn textures(value: &Value) -> bool {
        if value["kind"] == "texture" && value["value"].get("image").is_some() { return true; }
        match value { Value::Object(values) => values.values().any(textures), Value::Array(values) => values.iter().any(textures), _ => false }
    }
    assert!(records.iter().any(textures), "missing image texture");
    for record in &records {
        let data = &record["data"];
        match record["type"].as_str().unwrap() {
            "capy.occurrence/3" => {
                add("layer blend", data.get("blend").cloned().unwrap_or_else(|| wire::encode_layer_blend(crate::LayerBlend::Normal)));
                if let Some(value) = data.get("attachment") { add("attachment", value.clone()); }
            },
            "capy.paint-source/2" => {
                add("layer color mode", data["color_mode"].clone());
                if let Some(base) = data.get("base") { add("paint base policy", base.get("policy").cloned().unwrap_or(json!("source_profile"))); }
            },
            "capy.composition/2" => {
                let color = wire::parse_document_color(data.get("color").unwrap_or(&json!({}))).unwrap();
                add("composition profile", wire::encode_rgb_space(color.space)); add("composition depth", wire::encode_depth(color.depth));
                add("blend space", data.get("blend").cloned().unwrap_or_else(|| wire::encode_blend_space(crate::BlendSpace::Linear)));
                if let Some(resolution) = data.get("resolution") { add("resolution unit", resolution["unit"].clone()); }
            },
            "capy.image/1" => {
                let interpretation = &data["interpretation"];
                add("source channels", interpretation["channels"].clone()); add("source depth", interpretation["depth"].clone());
                if let Some(profile) = interpretation["profile"].get("builtin") { add("source profile", profile.clone()); }
                if interpretation["profile_assumed"] == true { add("assumed profile", json!(true)); }
            },
            "capy.image-object/1" => add("image interpolation", data.get("interpolation").cloned().unwrap_or(json!("linear"))),
            "capy.output/2" => if let Some(proof) = data.get("proof") { add("proof intent", proof.get("intent").cloned().unwrap_or_else(|| wire::encode_intent(RenderingIntent::RelativeColorimetric))); },
            "capy.guides/1" => if let Some(rulers) = data["rulers"].as_array() { for ruler in rulers { add("ruler geometry", ruler["geometry"]["kind"].clone()); } },
            "capy.selection/1" => for kind in ["pixels", "contours"] { if data["shape"].get(kind).is_some() { add("selection shape", json!(kind)); } },
            "capy.selection-coverage/1" => add("selection depth", data["depth"].clone()),
            _ => {},
        }
    }
    let require = |label: &str, value: Value| assert!(seen.get(label).is_some_and(|values| values.contains(&value.to_string())), "missing {label}: {value}");
    for blend in crate::LayerBlend::ALL { require("layer blend", wire::encode_layer_blend(blend)); }
    for value in ["clip","effect"] { require("attachment", json!(value)); }
    for value in [crate::color::LayerColorMode::FullColor,crate::color::LayerColorMode::Grayscale,crate::color::LayerColorMode::TwoTone] { require("layer color mode", serde_json::to_value(value).unwrap()); }
    for space in RgbSpace::ALL { require("composition profile", wire::encode_rgb_space(space)); require("source profile", wire::encode_rgb_space(space)); }
    for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] { require("composition depth", wire::encode_depth(depth)); require("source depth", wire::encode_depth(depth)); }
    for space in crate::BlendSpace::ALL { require("blend space", wire::encode_blend_space(space)); }
    for unit in [crate::ResolutionUnit::Inch,crate::ResolutionUnit::Centimetre,crate::ResolutionUnit::Metre] { require("resolution unit", wire::encode_resolution(crate::ImageResolution {unit,density:[[300,1];2]}).unwrap()["unit"].clone()); }
    for intent in [RenderingIntent::Perceptual,RenderingIntent::RelativeColorimetric,RenderingIntent::Saturation,RenderingIntent::AbsoluteColorimetric] { require("proof intent", wire::encode_intent(intent)); }
    for value in ["gray","gray_alpha","rgb","rgba","cmyk"] { require("source channels", json!(value)); }
    for (label, values) in [("image interpolation", ["linear","nearest"]),("paint base policy",["source_profile","working_pixels"]),("selection shape",["pixels","contours"]),("selection depth",["u4","u8"])] { for value in values { require(label, json!(value)); } }
    for value in ["straight","parallel","radial"] { require("ruler geometry", json!(value)); }
    require("assumed profile", json!(true));
    for number in vector_records::KINDS { assert!(kinds.contains(&number.word), "missing vector kind: {}", number.name); }
    for number in vector_records::TAGS { assert!(tags.contains(&number.word), "missing vector tag: {}", number.name); }
}
