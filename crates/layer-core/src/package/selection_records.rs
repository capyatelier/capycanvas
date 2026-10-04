use super::values::{self, DecodeError, DecodeResult};
use crate::{Affine, ProjectLimits, Selection, SelectionPixels, SelectionShape,
    authored::{PortableId, Resource}, selection::SELECTION_CHUNK_BYTES};
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub trait SelectionResourceWriter {
    fn validate_selection(&self, selection: &Selection) -> Result<(), String> { selection.validate().map_err(|e| e.to_string()) }
    fn chunk(&mut self, bytes: &Resource<[u8]>, descriptor: Value) -> Result<Value, String>;
    fn pixels(&mut self, pixels: &SelectionPixels) -> Result<Value, String> {
        let chunks = pixels.package_chunks()?.iter().enumerate().map(|(index, bytes)|
            self.chunk(bytes, descriptor(pixels, index))).collect::<Result<Vec<_>, _>>()?;
        Ok(json!({"extent":pixels.extent(), "bounds":pixels.bounds(),
            "depth":if pixels.coverage_format() == 2 {"u8"} else {"u4"}, "chunks":chunks}))
    }
}
pub trait SelectionResourceReader {
    fn validate_selection(&self, selection: &Selection) -> DecodeResult<()> { selection.validate().map_err(|e| e.to_string().into()) }
    fn chunk(&mut self, reference: &Value, expected_descriptor: &Value) -> DecodeResult<Resource<[u8]>>;
    fn limits(&self) -> ProjectLimits { ProjectLimits::default() }
    fn pixels(&mut self, extent: [u32; 2], bounds: [u32; 4], bytes: bool, chunks: Vec<Resource<[u8]>>) -> DecodeResult<Arc<SelectionPixels>> {
        Ok(Arc::new(SelectionPixels::from_package_chunks(extent, bounds, bytes, chunks)?))
    }
}

pub(crate) fn validate_selection_metadata(selection: &Selection) -> Result<(), String> {
    if selection.affine.inverse().is_none() { return Err("Invalid selection transform".into()); }
    if let SelectionShape::Pixels(pixels) = &selection.shape {
        let extent = pixels.extent(); let [x0, y0, x1, y1] = pixels.bounds();
        if extent.contains(&0) || extent.iter().any(|v| *v > crate::MAX_EXTENT)
            || x0 > x1 || y0 > y1 || x1 > extent[0] || y1 > extent[1]
            || u64::from(extent[0].div_ceil(pixels.pixels_per_word())) * u64::from(extent[1]) != pixels.words().len() as u64 {
            return Err("Invalid selection descriptor".into());
        }
        Ok(())
    } else { selection.validate().map_err(|e| e.to_string()) }
}

fn descriptor(pixels: &SelectionPixels, index: usize) -> Value {
    chunk_descriptor(pixels.extent(), pixels.bounds(), pixels.coverage_format() == 2, index)
}
pub(crate) fn chunk_descriptor(extent: [u32; 2], bounds: [u32; 4], bytes: bool, index: usize) -> Value {
    json!({"depth":if bytes {"u8"} else {"u4"}, "extent":extent, "bounds":bounds, "chunk":index})
}
fn integer_array<const N: usize>(value: &Value) -> DecodeResult<[u32; N]> {
    let mut result = [0; N];
    for (dest, value) in result.iter_mut().zip(values::array(value, N)?) { *dest = values::u32_value(value)?; }
    Ok(result)
}
fn reference(value: &Value) -> DecodeResult<()> {
    let fields = values::object(value, &["ref"])?;
    values::string(values::required(fields, "ref")?)?.parse::<PortableId>().map_err(DecodeError::from)?;
    Ok(())
}

pub fn encode_selection(selection: &Selection, resources: &mut impl SelectionResourceWriter) -> Result<Value, String> {
    resources.validate_selection(selection)?;
    let shape = match &selection.shape {
        SelectionShape::Contours(paths) => {
            let paths = paths.iter().map(|path| path.iter().copied().map(values::encode_point)
                .collect::<Result<Vec<_>, _>>().map(Value::Array)).collect::<Result<Vec<_>, _>>()?;
            json!({"contours":paths})
        },
        SelectionShape::Pixels(pixels) => json!({"pixels":resources.pixels(pixels)?}),
    };
    let mut result = Map::new();
    result.insert("shape".into(), shape);
    if selection.affine.0.map(f32::to_bits) != Affine::IDENTITY.0.map(f32::to_bits) {
        result.insert("affine".into(), Value::Array(selection.affine.0.into_iter().map(|v| Value::from(f64::from(v))).collect()));
    }
    if selection.inverted { result.insert("inverted".into(), Value::Bool(true)); }
    Ok(Value::Object(result))
}

pub fn decode_selection(value: &Value, resources: &mut impl SelectionResourceReader) -> DecodeResult<Selection> {
    let fields = values::object(value, &["shape", "affine", "inverted"])?;
    let affine = if let Some(value) = fields.get("affine") {
        let mut result = [0.; 6];
        for (dest, value) in result.iter_mut().zip(values::array(value, 6)?) { *dest = values::finite_f32(value)?; }
        Affine(result)
    } else { Affine::IDENTITY };
    if affine.inverse().is_none() { return Err("Invalid selection transform".into()); }
    let inverted = fields.get("inverted").map(values::boolean).transpose()?.unwrap_or(false);
    let shape = values::object(values::required(fields, "shape")?, &["contours", "pixels"])?;
    let shape = match (shape.get("contours"), shape.get("pixels")) {
        (Some(value), None) => {
            let contours = value.as_array().ok_or("Expected selection contours")?;
            let mut result = Vec::new();
            result.try_reserve_exact(contours.len()).map_err(|_| "Selection allocation failed")?;
            for contour in contours {
                let points = contour.as_array().ok_or("Expected a selection contour")?;
                if points.len() < 3 { return Err("Invalid selection contour".into()); }
                result.push(points.iter().map(values::parse_point).collect::<DecodeResult<Vec<_>>>()?.into());
            }
            SelectionShape::Contours(result.into())
        },
        (None, Some(value)) => {
            let fields = values::object(value, &["extent", "bounds", "depth", "chunks"])?;
            let extent: [u32; 2] = integer_array(values::required(fields, "extent")?)?;
            let bounds: [u32; 4] = integer_array(values::required(fields, "bounds")?)?;
            let byte_coverage = match values::string(values::required(fields, "depth")?)? {
                "u4" => false, "u8" => true,
                name => return Err(DecodeError::Unsupported(format!("Unknown selection depth {name}"))),
            };
            let limits = resources.limits();
            let [x0, y0, x1, y1] = bounds;
            let count = u64::from(extent[0].div_ceil(if byte_coverage { 4 } else { 8 })) * u64::from(extent[1]);
            let decoded_bytes = count.checked_mul(4).ok_or("Oversized selection coverage")?;
            let chunks = values::required(fields, "chunks")?.as_array().ok_or("Expected selection chunks")?;
            if extent.contains(&0) || extent.iter().any(|v| *v > limits.dimension || *v > crate::MAX_EXTENT)
                || x0 > x1 || y0 > y1 || x1 > extent[0] || y1 > extent[1]
                || decoded_bytes > limits.raster_bytes || usize::try_from(decoded_bytes).is_err()
                || chunks.len() > limits.tiles || decoded_bytes.div_ceil(SELECTION_CHUNK_BYTES as u64) != chunks.len() as u64 {
                return Err("Invalid or oversized selection coverage".into());
            }
            let mut loaded = Vec::new();
            loaded.try_reserve_exact(chunks.len()).map_err(|_| "Selection allocation failed")?;
            for (index, value) in chunks.iter().enumerate() {
                reference(value)?;
                loaded.push(resources.chunk(value, &chunk_descriptor(extent, bounds, byte_coverage, index))?);
            }
            SelectionShape::Pixels(resources.pixels(extent, bounds, byte_coverage, loaded)?)
        },
        _ => return Err("Selection shape requires exactly one alternative".into()),
    };
    let selection = Selection { shape, affine, inverted };
    resources.validate_selection(&selection)?;
    Ok(selection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Resources { chunks: BTreeMap<PortableId, (Resource<[u8]>, Value)>, reads: usize, limits: Option<ProjectLimits> }
    impl SelectionResourceWriter for Resources {
        fn chunk(&mut self, bytes: &Resource<[u8]>, descriptor: Value) -> Result<Value, String> {
            if let Some((previous, data)) = self.chunks.get(&bytes.id()) {
                assert!(previous.same_owner(bytes));
                assert_eq!(data, &descriptor);
            } else { self.chunks.insert(bytes.id(), (bytes.clone(), descriptor)); }
            Ok(json!({"ref":bytes.id().to_string()}))
        }
    }
    impl SelectionResourceReader for Resources {
        fn chunk(&mut self, value: &Value, expected: &Value) -> DecodeResult<Resource<[u8]>> {
            self.reads += 1;
            let id = value["ref"].as_str().unwrap().parse::<PortableId>().unwrap();
            let (bytes, data) = self.chunks.get(&id).ok_or("Missing selection chunk")?;
            if data != expected { return Err("Selection descriptor mismatch".into()); }
            Ok(bytes.clone())
        }
        fn limits(&self) -> ProjectLimits { self.limits.unwrap_or_default() }
    }
    fn pixels(extent: [u32; 2], byte_coverage: bool) -> Selection {
        let per_word = if byte_coverage { 4 } else { 8 };
        let count = (extent[0].div_ceil(per_word) * extent[1]) as usize;
        let mut words = vec![if byte_coverage { 0xff804020 } else { 0x43210432 }; count];
        if extent[0] % per_word != 0 {
            let mask = (1u64 << ((extent[0] % per_word) * (32 / per_word))) - 1;
            for word in words.iter_mut().skip(extent[0].div_ceil(per_word) as usize - 1).step_by(extent[0].div_ceil(per_word) as usize) {
                *word &= mask as u32;
            }
        }
        Selection::pixels(Arc::new(if byte_coverage {
            SelectionPixels::bytes(extent, [0, 0, extent[0], extent[1]], words)
        } else { SelectionPixels::new(extent, [0, 0, extent[0], extent[1]], words) }.unwrap()))
    }
    fn invalid(value: DecodeResult<Selection>) { assert!(matches!(value, Err(DecodeError::Invalid(_)))); }
    fn unsupported(value: DecodeResult<Selection>) { assert!(matches!(value, Err(DecodeError::Unsupported(_)))); }

    #[test]
    fn contours_preserve_geometry_placement_and_frozen_defaults() {
        let mut resources = Resources::default();
        assert_eq!(encode_selection(&Selection::empty(), &mut resources).unwrap(), json!({"shape":{"contours":[]}}));
        for selection in [Selection::empty(), Selection::full(), Selection {
            shape: SelectionShape::Contours(vec![vec![crate::Point{x:-0.,y:0.}, crate::Point{x:1.125,y:0.},
                crate::Point{x:0.,y:1.}].into()].into()), affine:Affine([1.,-0.,0.25,2.,3.125,4.]), inverted:true }] {
            let wire = encode_selection(&selection, &mut resources).unwrap();
            let wire: Value = serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
            let restored = decode_selection(&wire, &mut resources).unwrap();
            assert_eq!(restored, selection);
            assert_eq!(restored.affine.0.map(f32::to_bits), selection.affine.0.map(f32::to_bits));
            if !selection.contours().is_empty() { assert_eq!(restored.contours()[0][0].x.to_bits(), (-0f32).to_bits()); }
        }
        assert!(resources.chunks.is_empty());
    }

    #[test]
    fn compressed_chunk_identity_bytes_and_ownership_survive_captures_and_open() {
        for byte_coverage in [false, true] {
            let selection = pixels([1033, 257], byte_coverage);
            let SelectionShape::Pixels(pixels) = &selection.shape else { panic!() };
            let capture_before_encoding = pixels.as_ref().clone();
            let mut resources = Resources::default();
            let wire = encode_selection(&selection, &mut resources).unwrap();
            assert_eq!(encode_selection(&selection.clone(), &mut resources).unwrap(), wire);
            assert!(pixels.package_chunks().unwrap()[0].same_owner(&capture_before_encoding.package_chunks().unwrap()[0]));
            assert_eq!(resources.chunks.len(), (pixels.words().len() * 4).div_ceil(SELECTION_CHUNK_BYTES));
            let restored = decode_selection(&wire, &mut resources).unwrap();
            assert_eq!(restored, selection);
            assert_eq!(encode_selection(&restored, &mut resources).unwrap(), wire);
            let SelectionShape::Pixels(restored) = restored.shape else { panic!() };
            for (original, loaded) in pixels.package_chunks().unwrap().iter().zip(restored.package_chunks().unwrap()) {
                assert_eq!(original.id(), loaded.id());
                assert!(Arc::ptr_eq(original.storage(), loaded.storage()));
            }
            let legacy = serde_json::to_value(pixels.as_ref()).unwrap();
            assert_eq!(legacy.as_object().unwrap().len(), 4);
            let uncached: SelectionPixels = serde_json::from_value(legacy).unwrap();
            assert_eq!(&uncached, pixels.as_ref());
            assert_ne!(uncached.package_chunks().unwrap()[0].id(), pixels.package_chunks().unwrap()[0].id());
        }
    }

    #[test]
    fn loaded_chunks_retain_noncanonical_compressed_bytes_and_explicit_ids() {
        let selection = pixels([5, 1], true);
        let SelectionShape::Pixels(pixels) = selection.shape else { panic!() };
        let mut bytes = vec![0; SELECTION_CHUNK_BYTES];
        for (word, dest) in pixels.words().iter().zip(bytes.chunks_exact_mut(4)) { dest.copy_from_slice(&word.to_le_bytes()); }
        let mut literal = vec![0xf0];
        let mut length = bytes.len() - 15;
        while length >= 255 { literal.push(255); length -= 255; }
        literal.push(length as u8);
        literal.extend(bytes);
        assert_ne!(literal, &*pixels.package_chunks().unwrap()[0]);
        let id = PortableId::from_bytes([7; 16]);
        let chunk = Resource::with_id(id, Arc::<[u8]>::from(literal));
        let restored = SelectionPixels::from_package_chunks([5,1], [0,0,5,1], true, vec![chunk.clone()]).unwrap();
        assert_eq!(&restored, pixels.as_ref());
        assert!(restored.package_chunks().unwrap()[0].same_owner(&chunk));
        assert_eq!(restored.package_chunks().unwrap()[0].id(), id);
    }

    #[test]
    fn closed_selection_grammar_classifies_invalid_values_and_unknown_additions() {
        let mut resources = Resources::default();
        for wire in [json!({}), json!({"shape":{}}), json!({"shape":{"pixels":{},"contours":[]}}),
            json!({"shape":{"contours":[[[0,0],[1,1]]]}}), json!({"shape":{"contours":[]},"inverted":0}),
            json!({"shape":{"contours":[]},"affine":[0,0,0,0,0,0]}),
            json!({"shape":{"contours":[[[0,0],[1e100,0],[0,1]]]}})] {
            invalid(decode_selection(&wire, &mut resources));
        }
        for wire in [json!({"shape":{"contours":[]},"future":0}), json!({"shape":{"future":{}}}),
            json!({"shape":{"pixels":{"extent":[8,1],"bounds":[0,0,8,1],"depth":"f32","chunks":[]}}})] {
            unsupported(decode_selection(&wire, &mut resources));
        }
        let mut wire = encode_selection(&pixels([8,1], false), &mut resources).unwrap();
        wire["shape"]["pixels"]["chunks"][0]["future"] = json!(true);
        unsupported(decode_selection(&wire, &mut resources));
        assert_eq!(resources.reads, 0);
    }

    #[test]
    fn selection_allocation_and_descriptor_limits_precede_resource_reads() {
        let mut resources = Resources::default();
        let wire = encode_selection(&pixels([8,1], false), &mut resources).unwrap();
        for limits in [ProjectLimits{raster_bytes:3,..Default::default()},
            ProjectLimits{tiles:0,..Default::default()}, ProjectLimits{dimension:7,..Default::default()}] {
            resources.limits = Some(limits);
            invalid(decode_selection(&wire, &mut resources));
            assert_eq!(resources.reads, 0);
        }
        resources.limits = None;
        let mut missing = wire.clone();
        missing["shape"]["pixels"]["chunks"] = json!([]);
        invalid(decode_selection(&missing, &mut resources));
        assert_eq!(resources.reads, 0);
        resources.chunks.values_mut().next().unwrap().1["chunk"] = json!(1);
        invalid(decode_selection(&wire, &mut resources));
    }

    #[test]
    fn malformed_words_bounds_padding_and_compression_are_rejected() {
        fn load(raw: Vec<u8>, extent: [u32;2], bounds: [u32;4], bytes: bool) -> Result<SelectionPixels,String> {
            SelectionPixels::from_package_chunks(extent,bounds,bytes,vec![Resource::from(lz4_flex::block::compress(&raw))])
        }
        let mut raw = vec![0; SELECTION_CHUNK_BYTES];
        raw[0] = 5;
        assert!(load(raw.clone(),[8,1],[0,0,8,1],false).is_err());
        raw[0] = 1;
        assert!(load(raw.clone(),[8,1],[1,0,8,1],false).is_err());
        raw[0] = 0;
        raw[1] = 1;
        assert!(load(raw.clone(),[1,1],[0,0,1,1],true).is_err());
        raw[1] = 0;
        raw[4] = 1;
        assert!(load(raw.clone(),[1,1],[0,0,1,1],true).is_err());
        assert!(load(vec![0;SELECTION_CHUNK_BYTES-1],[8,1],[0,0,8,1],false).is_err());
        let raw = vec![0;SELECTION_CHUNK_BYTES];
        let compressed = lz4_flex::block::compress(&raw);
        for bytes in [compressed[..compressed.len()-1].to_vec(),
            [compressed.clone(),vec![0]].concat(),[compressed.clone(),compressed].concat(),vec![0x10,42,0,0,0]] {
            assert!(SelectionPixels::from_package_chunks([8,1],[0,0,8,1],false,vec![Resource::from(bytes)]).is_err());
        }
        let omitted = SelectionPixels::new([8,1],[1,0,8,1],vec![1]).unwrap();
        assert!(omitted.package_chunks().is_err());
        assert!(SelectionPixels::from_package_chunks([8,1],[0,0,8,1],false,vec![]).is_err());
    }
}
