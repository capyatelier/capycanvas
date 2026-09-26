//! JPEG gain-map interchange: MPF image offsets, ISO 21496-1 fractions and
//! Adobe HDR gain-map XMP. Codecs operate only on the validated image slices.
use super::super::jpeg_markers::{Segment, scan};
use super::super::jpeg_mpf::mpf_entries;
use super::{GainMapMetadata, Metadata};
use quick_xml::{events::Event, name::ResolveResult, reader::NsReader};
use std::collections::BTreeMap;

const ISO: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
const XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const HDRGM: &[u8] = b"http://ns.adobe.com/hdr-gain-map/1.0/";
const INVALID: &str = "Invalid JPEG gain-map metadata";

fn iso_metadata(bytes: &[u8]) -> Result<Metadata, String> {
    Metadata::parse_iso(bytes, true)?.ok_or_else(|| "Unsupported ISO gain-map version".into())
}

fn xmp_metadata(bytes: &[u8]) -> Result<Option<Metadata>, String> {
    let mut reader = NsReader::from_reader(bytes);
    let mut fields = BTreeMap::<String, String>::new();
    let mut active = None::<(String, usize)>;
    let mut depth = 0usize;
    loop {
        let event = reader.read_event().map_err(|_| INVALID)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let (ns, local) = reader.resolver().resolve_element(e.name());
                if matches!(ns, ResolveResult::Bound(ns) if ns.as_ref() == HDRGM) {
                    let key = std::str::from_utf8(local.as_ref())
                        .map_err(|_| INVALID)?
                        .to_owned();
                    if fields.insert(key.clone(), String::new()).is_some() || active.is_some() {
                        return Err(INVALID.into());
                    }
                    if matches!(event, Event::Start(_)) {
                        active = Some((key, depth));
                    }
                }
                for attr in e.attributes() {
                    let attr = attr.map_err(|_| INVALID)?;
                    let (ns, local) = reader.resolver().resolve_attribute(attr.key);
                    if matches!(ns, ResolveResult::Bound(ns) if ns.as_ref() == HDRGM) {
                        let key = std::str::from_utf8(local.as_ref())
                            .map_err(|_| INVALID)?
                            .to_owned();
                        let value = attr
                            .decoded_and_normalized_value(
                                quick_xml::XmlVersion::Implicit1_0,
                                reader.decoder(),
                            )
                            .map_err(|_| INVALID)?
                            .into_owned();
                        if fields.insert(key, value).is_some() {
                            return Err(INVALID.into());
                        }
                    }
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                    if depth > 32 {
                        return Err(INVALID.into());
                    }
                }
            }
            Event::Text(e) => {
                if let Some((key, _)) = &active {
                    let text = e
                        .xml_content(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| INVALID)?;
                    let value = quick_xml::escape::unescape(&text).map_err(|_| INVALID)?;
                    let field = fields.get_mut(key).unwrap();
                    field.push_str(&value);
                    field.push(' ');
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or(INVALID)?;
                if active.as_ref().is_some_and(|(_, d)| *d == depth) {
                    active = None;
                }
            }
            Event::DocType(_) | Event::GeneralRef(_) | Event::CData(_) => {
                return Err(INVALID.into());
            }
            Event::Eof => break,
            _ => (),
        }
    }
    if depth != 0 {
        return Err(INVALID.into());
    }
    if !fields.contains_key("Version") {
        return Ok(None);
    }
    if fields["Version"].trim() != "1.0" {
        return Err("Unsupported XMP gain-map version".into());
    }
    if fields
        .get("BaseRenditionIsHDR")
        .is_some_and(|v| v.trim() != "False")
    {
        return Err("HDR-base JPEG gain maps are not supported".into());
    }
    let array = |name: &str, default: f32| -> Result<[f32; 3], String> {
        let Some(value) = fields.get(name) else {
            return Ok([default; 3]);
        };
        let values: Vec<f32> = value
            .split_whitespace()
            .map(|v| v.parse().map_err(|_| INVALID))
            .collect::<Result<_, _>>()?;
        match values.as_slice() {
            [v] => Ok([*v; 3]),
            [r, g, b] => Ok([*r, *g, *b]),
            _ => Err(INVALID.into()),
        }
    };
    let scalar = |name: &str, default| -> Result<f32, String> {
        let v = array(name, default)?;
        if v[0] != v[1] || v[0] != v[2] {
            return Err(INVALID.into());
        }
        Ok(v[0])
    };
    Ok(Some(
        Metadata {
            min: array("GainMapMin", 0.)?,
            max: array("GainMapMax", 1.)?,
            gamma: array("Gamma", 1.)?,
            base_offset: array("OffsetSDR", 1. / 64.)?,
            alternate_offset: array("OffsetHDR", 1. / 64.)?,
            base_headroom: scalar("HDRCapacityMin", 0.)?,
            alternate_headroom: scalar("HDRCapacityMax", 1.)?,
            use_base_space: true,
        }
        .validate()?,
    ))
}

fn metadata(segments: &[Segment<'_>]) -> Result<Option<Metadata>, String> {
    let mut iso = None;
    for s in segments {
        if s.marker == 0xe2 {
            if let Some(bytes) = s.bytes.strip_prefix(ISO) {
                if iso.replace(iso_metadata(bytes)?).is_some() {
                    return Err(INVALID.into());
                }
            }
        }
    }
    // ISO metadata takes precedence, including when an older XMP packet is
    // stale or uses a representation this implementation does not understand.
    if iso.is_some() {
        return Ok(iso);
    }
    let mut xmp = None;
    for s in segments.iter().filter(|s| s.marker == 0xe1) {
        if let Some(bytes) = s.bytes.strip_prefix(XMP) {
            if bytes.windows(HDRGM.len()).any(|w| w == HDRGM) {
                if let Some(value) = xmp_metadata(bytes)? {
                    if xmp.replace(value).is_some() {
                        return Err(INVALID.into());
                    }
                }
            }
        }
    }
    Ok(xmp)
}

pub(super) struct Images<'a> {
    pub base: &'a [u8],
    pub gain: &'a [u8],
    pub metadata: Metadata,
}
pub(super) fn parse(bytes: &[u8]) -> Result<Images<'_>, String> {
    let (base_end, segments) = scan(bytes)?;
    let mut directories = segments
        .iter()
        .filter(|s| s.marker == 0xe2 && s.bytes.starts_with(b"MPF\0"));
    let mpf = directories
        .next()
        .ok_or("JPEG gain map has no MPF directory")?;
    if directories.next().is_some() {
        return Err(INVALID.into());
    }
    let entries = mpf_entries(&mpf.bytes[4..])?;
    if entries.len() < 2 {
        return Err(INVALID.into());
    }
    let mut selected = None;
    let mut previous_end = base_end;
    for (index, [_, size, offset]) in entries.into_iter().enumerate() {
        let (size, offset) = (size as usize, offset as usize);
        if index == 0 {
            if offset != 0 || size != base_end {
                return Err(INVALID.into());
            }
            continue;
        }
        let start = (mpf.offset + 4).checked_add(offset).ok_or(INVALID)?;
        let end = start.checked_add(size).ok_or(INVALID)?;
        if start < previous_end {
            return Err(INVALID.into());
        }
        let image = bytes.get(start..end).ok_or(INVALID)?;
        let (image_end, segments) = scan(image)?;
        if image_end != size {
            return Err(INVALID.into());
        }
        previous_end = end;
        if let Some(metadata) = metadata(&segments)? {
            if selected
                .replace(Images {
                    base: &bytes[..base_end],
                    gain: image,
                    metadata,
                })
                .is_some()
            {
                return Err("Multiple JPEG gain maps are not supported".into());
            }
        }
    }
    selected.ok_or("JPEG gain-map image or metadata is missing".into())
}

fn segment(marker: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
    let size = u16::try_from(payload.len() + 2).map_err(|_| INVALID)?;
    Ok([&[0xff, marker], size.to_be_bytes().as_slice(), payload].concat())
}
fn xmp(description: &str) -> Result<Vec<u8>, String> {
    let xml = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">{description}</rdf:RDF></x:xmpmeta>"
    );
    segment(0xe1, &[XMP, xml.as_bytes()].concat())
}

pub(super) fn assemble(base: &[u8], gain: &[u8], m: GainMapMetadata) -> Result<Vec<u8>, String> {
    if scan(base)?.0 != base.len() || scan(gain)?.0 != gain.len() {
        return Err(INVALID.into());
    }
    let iso_gain = segment(0xe2, &[ISO, &m.iso_bytes(true)?].concat())?;
    let xmp_gain = xmp(&format!(
        "<rdf:Description xmlns:hdrgm=\"http://ns.adobe.com/hdr-gain-map/1.0/\" hdrgm:Version=\"1.0\" hdrgm:GainMapMin=\"{}\" hdrgm:GainMapMax=\"{}\" hdrgm:Gamma=\"1\" hdrgm:OffsetSDR=\"{}\" hdrgm:OffsetHDR=\"{}\" hdrgm:HDRCapacityMin=\"0\" hdrgm:HDRCapacityMax=\"{}\" hdrgm:BaseRenditionIsHDR=\"False\"/>",
        m.min_log2, m.max_log2, m.offset, m.offset, m.headroom
    ))?;
    let gain = [b"\xff\xd8".as_slice(), &iso_gain, &xmp_gain, &gain[2..]].concat();
    let iso_base = segment(0xe2, &[ISO, &[0, 0, 0, 0]].concat())?;
    let xmp_base = xmp(&format!(
        "<rdf:Description xmlns:Container=\"http://ns.google.com/photos/1.0/container/\" xmlns:Item=\"http://ns.google.com/photos/1.0/container/item/\" xmlns:hdrgm=\"http://ns.adobe.com/hdr-gain-map/1.0/\" hdrgm:Version=\"1.0\"><Container:Directory><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"Primary\" Item:Mime=\"image/jpeg\"/></rdf:li><rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"GainMap\" Item:Mime=\"image/jpeg\" Item:Length=\"{}\"/></rdf:li></rdf:Seq></Container:Directory></rdf:Description>",
        gain.len()
    ))?;
    let primary_len = base.len() + 90 + iso_base.len() + xmp_base.len();
    let mut mpf = b"MPF\0MM\0\x2a\0\0\0\x08\0\x03".to_vec();
    for (tag, ty, count, value) in [
        (0xb000u16, 7u16, 4u32, 0x30313030u32),
        (0xb001, 4, 1, 2),
        (0xb002, 7, 32, 50),
    ] {
        mpf.extend(tag.to_be_bytes());
        mpf.extend(ty.to_be_bytes());
        mpf.extend(count.to_be_bytes());
        mpf.extend(value.to_be_bytes());
    }
    mpf.extend(0u32.to_be_bytes());
    // MPF is the first marker: TIFF origin is at byte 10 in the output file.
    for (attributes, size, offset) in [
        (0x00030000u32, primary_len, 0),
        (0, gain.len(), primary_len - 10),
    ] {
        mpf.extend(attributes.to_be_bytes());
        mpf.extend(u32::try_from(size).map_err(|_| INVALID)?.to_be_bytes());
        mpf.extend(u32::try_from(offset).map_err(|_| INVALID)?.to_be_bytes());
        mpf.extend([0; 4]);
    }
    let mpf = segment(0xe2, &mpf)?;
    debug_assert_eq!(mpf.len(), 90);
    Ok([
        b"\xff\xd8".as_slice(),
        &mpf,
        &iso_base,
        &xmp_base,
        &base[2..],
        &gain,
    ]
    .concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xmp_resolves_namespaces_and_channel_arrays() {
        let xml = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:g="http://ns.adobe.com/hdr-gain-map/1.0/" g:Version="1.0" g:HDRCapacityMax="3"><g:GainMapMax><rdf:Seq><rdf:li>1</rdf:li><rdf:li>2</rdf:li><rdf:li>3</rdf:li></rdf:Seq></g:GainMapMax><g:Gamma>2</g:Gamma></rdf:Description></rdf:RDF>"#;
        let m = xmp_metadata(xml).unwrap().unwrap();
        assert_eq!(m.max, [1., 2., 3.]);
        assert_eq!(m.gamma, [2.; 3]);
        let h = m.reconstruct([0.5; 3], [0.25; 3]);
        for c in 0..3 {
            assert!(
                (h[c] - ((0.5 + 1. / 64.) * (0.5 * (c + 1) as f32).exp2() - 1. / 64.)).abs() < 1e-6
            );
        }
        let invalid = String::from_utf8(xml.to_vec())
            .unwrap()
            .replace("<g:Gamma>2", "<g:Gamma>NaN");
        assert!(xmp_metadata(invalid.as_bytes()).is_err());
    }
    #[test]
    fn iso_rejects_truncation_zero_denominators_and_unsupported_direction() {
        let mut bytes = vec![0, 0, 0, 0, 0x48];
        bytes.extend(64u32.to_be_bytes());
        for v in [0i32, 256, -128, 512, 64, 1, 1] {
            bytes.extend(v.to_be_bytes());
        }
        let m = iso_metadata(&bytes).unwrap();
        assert_eq!(m.min, [-2.; 3]);
        assert_eq!(m.max, [8.; 3]);
        for len in 0..bytes.len() {
            assert!(iso_metadata(&bytes[..len]).is_err());
        }
        let mut zero = bytes.clone();
        zero[5..9].fill(0);
        assert!(iso_metadata(&zero).is_err());
        bytes[4] |= 4;
        assert!(iso_metadata(&bytes).is_err());
    }
}
