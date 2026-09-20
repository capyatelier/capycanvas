//! JPEG gain-map interchange: MPF image offsets, ISO 21496-1 fractions and
//! Adobe HDR gain-map XMP. Codecs operate only on the validated image slices.
use super::{GainMapMetadata, Metadata};
use quick_xml::{events::Event, name::ResolveResult, reader::NsReader};
use std::collections::BTreeMap;

const ISO: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
const XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const HDRGM: &[u8] = b"http://ns.adobe.com/hdr-gain-map/1.0/";
const INVALID: &str = "Invalid JPEG gain-map metadata";

fn iso_metadata(bytes: &[u8]) -> Result<Metadata, String> {
    let mut bytes = bytes;
    fn take<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N], String> {
        let (head, tail) = bytes.split_at_checked(N).ok_or(INVALID)?;
        *bytes = tail;
        Ok(head.try_into().unwrap())
    }
    if u16::from_be_bytes(take(&mut bytes)?) != 0 {
        return Err("Unsupported ISO gain-map version".into());
    }
    let _writer = u16::from_be_bytes(take(&mut bytes)?);
    let flags = take::<1>(&mut bytes)?[0];
    if flags & 4 != 0 {
        return Err("HDR-base JPEG gain maps are not supported".into());
    }
    if flags & 0x33 != 0 {
        return Err(INVALID.into());
    }
    let common = if flags & 8 != 0 {
        Some(u32::from_be_bytes(take(&mut bytes)?))
    } else {
        None
    };
    let mut fraction = |signed| -> Result<f32, String> {
        let bits = take(&mut bytes)?;
        let n = if signed {
            i32::from_be_bytes(bits) as f64
        } else {
            u32::from_be_bytes(bits) as f64
        };
        let d = match common {
            Some(d) => d,
            None => u32::from_be_bytes(take(&mut bytes)?),
        };
        if d == 0 {
            return Err(INVALID.into());
        }
        Ok((n / f64::from(d)) as f32)
    };
    let mut m = Metadata {
        min: [0.; 3],
        max: [0.; 3],
        gamma: [1.; 3],
        base_offset: [0.; 3],
        alternate_offset: [0.; 3],
        base_headroom: fraction(false)?,
        alternate_headroom: fraction(false)?,
        use_base_space: flags & 0x40 != 0,
    };
    let channels = if flags & 0x80 != 0 { 3 } else { 1 };
    for c in 0..channels {
        m.min[c] = fraction(true)?;
        m.max[c] = fraction(true)?;
        m.gamma[c] = fraction(false)?;
        m.base_offset[c] = fraction(true)?;
        m.alternate_offset[c] = fraction(true)?;
    }
    if channels == 1 {
        for v in [
            &mut m.min,
            &mut m.max,
            &mut m.gamma,
            &mut m.base_offset,
            &mut m.alternate_offset,
        ] {
            let first = v[0];
            v.fill(first);
        }
    }
    m.validate()
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

struct Segment<'a> {
    marker: u8,
    offset: usize,
    bytes: &'a [u8],
}
fn scan(bytes: &[u8]) -> Result<(usize, Vec<Segment<'_>>), String> {
    if !bytes.starts_with(b"\xff\xd8") {
        return Err("Invalid gain-map JPEG signature".into());
    }
    let mut at = 2usize;
    let mut entropy = false;
    let mut segments = Vec::new();
    let mut scans = 0;
    loop {
        if entropy {
            at += bytes
                .get(at..)
                .and_then(|b| b.iter().position(|v| *v == 0xff))
                .ok_or(INVALID)?;
        }
        if bytes.get(at) != Some(&0xff) {
            return Err(INVALID.into());
        }
        while bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *bytes.get(at).ok_or(INVALID)?;
        at += 1;
        if entropy && (marker == 0 || (0xd0..=0xd7).contains(&marker)) {
            continue;
        }
        if marker == 0xd9 {
            return Ok((at, segments));
        }
        if marker == 1 {
            continue;
        }
        if marker == 0 || (0xd0..=0xd8).contains(&marker) {
            return Err(INVALID.into());
        }
        let size =
            u16::from_be_bytes(bytes.get(at..at + 2).ok_or(INVALID)?.try_into().unwrap()) as usize;
        if size < 2 {
            return Err(INVALID.into());
        }
        let data = bytes.get(at + 2..at + size).ok_or(INVALID)?;
        if matches!(marker, 0xe1 | 0xe2) {
            if segments.len() >= 4096 {
                return Err("Too many JPEG metadata segments".into());
            }
            segments.push(Segment {
                marker,
                offset: at + 2,
                bytes: data,
            });
        }
        at += size;
        entropy = marker == 0xda;
        if entropy {
            scans += 1;
            if scans > 256 {
                return Err("Too many JPEG scans".into());
            }
        }
    }
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
    let mut selected = None;
    let mut previous_end = base_end;
    for (index, (size, offset)) in entries.into_iter().enumerate() {
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

fn mpf_entries(bytes: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    let little = match bytes.get(..4) {
        Some(b"II\x2a\0") => true,
        Some(b"MM\0\x2a") => false,
        _ => return Err(INVALID.into()),
    };
    let u16_at = |at: usize| -> Result<u16, String> {
        let b = bytes
            .get(at..at.checked_add(2).ok_or(INVALID)?)
            .ok_or(INVALID)?
            .try_into()
            .unwrap();
        Ok(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |at: usize| -> Result<usize, String> {
        let b = bytes
            .get(at..at.checked_add(4).ok_or(INVALID)?)
            .ok_or(INVALID)?
            .try_into()
            .unwrap();
        Ok(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        } as usize)
    };
    let ifd = u32_at(4)?;
    if ifd < 8 {
        return Err(INVALID.into());
    }
    let count = u16_at(ifd)? as usize;
    let end = ifd.checked_add(2 + 12 * count + 4).ok_or(INVALID)?;
    if end > bytes.len() {
        return Err(INVALID.into());
    }
    let mut images = None;
    let mut entries = None;
    for i in 0..count {
        let at = ifd + 2 + 12 * i;
        match u16_at(at)? {
            0xb001 => {
                if u16_at(at + 2)? != 4
                    || u32_at(at + 4)? != 1
                    || images.replace(u32_at(at + 8)?).is_some()
                {
                    return Err(INVALID.into());
                }
            }
            0xb002 => {
                if u16_at(at + 2)? != 7
                    || entries
                        .replace((u32_at(at + 8)?, u32_at(at + 4)?))
                        .is_some()
                {
                    return Err(INVALID.into());
                }
            }
            _ => (),
        }
    }
    let images = images.ok_or(INVALID)?;
    let (at, size) = entries.ok_or(INVALID)?;
    if !(2..=64).contains(&images)
        || size != images * 16
        || at < end
        || at.checked_add(size).is_none_or(|v| v > bytes.len())
    {
        return Err(INVALID.into());
    }
    (0..images)
        .map(|i| {
            let at = at + i * 16;
            if u32_at(at)? & 0x07000000 != 0 {
                return Err("Unsupported MPF image encoding".into());
            }
            Ok((u32_at(at + 4)?, u32_at(at + 8)?))
        })
        .collect()
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
    let mut iso = ISO.to_vec();
    iso.extend([0, 0, 0, 0, 0x48]); // version 0, base color space, common denominator
    let denominator = 1_000_000u32;
    iso.extend(denominator.to_be_bytes());
    for v in [
        0., m.headroom, m.min_log2, m.max_log2, 1., m.offset, m.offset,
    ] {
        if !v.is_finite() || (v as f64 * f64::from(denominator)).abs() > i32::MAX as f64 {
            return Err(INVALID.into());
        }
        iso.extend(((v as f64 * f64::from(denominator)).round() as i32).to_be_bytes());
    }
    iso_metadata(&iso[ISO.len()..])?;
    let iso_gain = segment(0xe2, &iso)?;
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
