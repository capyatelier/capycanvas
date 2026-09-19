//! First AVIS sample selection. Tables remain borrowed and counts are checked
//! against their actual bytes; importing one frame never expands a timeline.
use super::{
    RawImage, codec,
    container::{Container, Reader, Result, boxes},
    properties::Properties,
};
use std::sync::atomic::AtomicBool;

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Result<Option<&'a [u8]>> {
    let mut found = None;
    for view in boxes(data) {
        let view = view?;
        if &view.kind == kind && found.replace(view.data).is_some() {
            return Err("Duplicate AVIF sequence box".into());
        }
    }
    Ok(found)
}
fn required<'a>(data: &'a [u8], kind: &[u8; 4]) -> Result<&'a [u8]> {
    child(data, kind)?
        .ok_or_else(|| format!("Missing AVIF sequence {}", String::from_utf8_lossy(kind)))
}
fn table(bytes: &[u8], width: usize) -> Result<(u32, Reader<'_>)> {
    let mut r = Reader::new(bytes);
    if r.full()? != (0, 0) {
        return Err("Unsupported AVIF sample table version".into());
    }
    let count = r.u32()?;
    if count as usize > r.left() / width || count as usize * width != r.left() {
        return Err("Invalid AVIF sample table count".into());
    }
    Ok((count, r))
}
struct Track<'a> {
    id: u32,
    alpha_of: Option<u32>,
    prem_to: Option<u32>,
    pub properties: Properties<'a>,
    sample: &'a [u8],
    metadata: Option<&'a [u8]>,
}
pub(super) struct Sequence<'a> {
    color: Track<'a>,
    alpha: Option<Track<'a>>,
}
impl<'a> Sequence<'a> {
    pub fn parse(container: &Container<'a>, cancel: &AtomicBool) -> Result<Option<Self>> {
        let Some(movie) = container.movie else {
            return Ok(None);
        };
        let mut tracks = Vec::new();
        for view in boxes(movie) {
            codec::check(cancel)?;
            let view = view?;
            if &view.kind != b"trak" {
                continue;
            }
            if tracks.len() == 32 {
                return Err("Too many AVIF image tracks".into());
            }
            if let Some(track) = Track::parse(container.file, view.data)? {
                tracks.push(track);
            }
        }
        let Some(index) = tracks.iter().position(|v| v.alpha_of.is_none()) else {
            return Ok(None);
        };
        let color = tracks.remove(index);
        let mut matching = tracks.into_iter().filter(|v| v.alpha_of == Some(color.id));
        let alpha = matching.next();
        if matching.next().is_some() {
            return Err("Multiple AVIF alpha tracks".into());
        }
        if let Some(alpha) = &alpha {
            if alpha.properties.extent != color.properties.extent {
                return Err("AVIF alpha track dimensions disagree".into());
            }
        }
        Ok(Some(Self { color, alpha }))
    }
    pub fn properties(&self) -> &Properties<'a> {
        &self.color.properties
    }
    pub fn decode(&self, budget: usize, cancel: &AtomicBool) -> Result<RawImage> {
        let p = &self.color.properties;
        let mut image = super::decode_coded(self.color.sample, p, p.color, false, budget, cancel)?;
        if let Some(alpha) = &self.alpha {
            let remaining = budget
                .checked_sub(image.pixels.len() * 8)
                .ok_or("AVIF alpha track exceeds the codec budget")?;
            let alpha_image = super::decode_coded(
                alpha.sample,
                &alpha.properties,
                None,
                true,
                remaining,
                cancel,
            )?;
            if alpha_image.depth != image.depth || alpha_image.extent != image.extent {
                return Err("AVIF alpha track precision or extent mismatch".into());
            }
            for (i, (pixel, alpha)) in image.pixels.iter_mut().zip(&alpha_image.pixels).enumerate()
            {
                if i % image.extent[0] as usize == 0 {
                    codec::check(cancel)?;
                }
                pixel[3] = alpha[0];
            }
            image.premultiplied = self.color.prem_to == Some(alpha.id);
        }
        Ok(image)
    }
    pub fn resolution(
        &self,
        container: &Container<'a>,
        budget: usize,
        cancel: &AtomicBool,
    ) -> Result<Option<layer_core::ImageResolution>> {
        let Some(meta) = self.color.metadata else {
            return Ok(None);
        };
        let metadata = Container::track_metadata(container.file, meta, budget, cancel)?;
        let mut result = None;
        for item in &metadata.items {
            if &item.kind != b"Exif" {
                continue;
            }
            let payload = metadata.payload(
                item.id,
                crate::MAX_ICC_BYTES.min(budget.saturating_sub(metadata.metadata_bytes)),
            )?;
            let mut r = Reader::new(&payload);
            let offset = r.u32()? as usize;
            r.take(offset)?;
            result = super::super::metadata::exif(r.data)?.resolution;
        }
        Ok(result)
    }
}
impl<'a> Track<'a> {
    fn parse(file: &'a [u8], track: &'a [u8]) -> Result<Option<Self>> {
        let mut header = Reader::new(required(track, b"tkhd")?);
        let (version, flags) = header.full()?;
        if version > 1 {
            return Err("Invalid AVIF track header version".into());
        }
        header.take(if version == 0 { 8 } else { 16 })?;
        let id = header.u32()?;
        header.take(if version == 0 { 24 } else { 28 })?;
        for expected in [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000] {
            if header.u32()? != expected {
                return Err("AVIF track transformations require a still-image export".into());
            }
        }
        let display = [header.u32()?, header.u32()?];
        header.end()?;
        let media = required(track, b"mdia")?;
        let mut handler = Reader::new(required(media, b"hdlr")?);
        if handler.full()? != (0, 0) {
            return Err("Invalid AVIF track handler".into());
        }
        handler.take(4)?;
        if !matches!(handler.take(4)?, b"pict" | b"auxv") || flags & 1 == 0 {
            return Ok(None);
        }
        if let Some(edits) = child(track, b"edts")? {
            let mut r = Reader::new(required(edits, b"elst")?);
            let (v, flags) = r.full()?;
            // Flag 1 repeats this edit; it does not change the first sample.
            if v > 1 || flags > 1 || r.u32()? != 1 {
                return Err("Unsupported AVIF sequence edit list".into());
            }
            r.take(if v == 0 { 4 } else { 8 })?;
            let time = if v == 0 { r.u32()? as u64 } else { r.u64()? };
            if time != 0 || r.u32()? != 0x10000 {
                return Err("AVIF sequence edits require a still-image export".into());
            }
            r.end()?;
        }
        let sample_table = required(required(media, b"minf")?, b"stbl")?;
        let mut descriptions = Reader::new(required(sample_table, b"stsd")?);
        if descriptions.full()? != (0, 0) || descriptions.u32()? != 1 {
            return Err("Unsupported AVIF sample descriptions".into());
        }
        let mut entries = boxes(descriptions.data);
        let entry = entries.next().ok_or("Missing AVIF sample entry")??;
        if entries.next().is_some() || &entry.kind != b"av01" {
            return Err("Unsupported AVIF sample entry".into());
        }
        let mut entry_header = Reader::new(entry.data);
        entry_header.take(6)?;
        if entry_header.u16()? != 1 {
            return Err("Unsupported AVIF track data reference".into());
        }
        entry_header.take(16)?;
        let extent = [entry_header.u16()? as u32, entry_header.u16()? as u32];
        if display != [0, 0] && display != [extent[0] << 16, extent[1] << 16] {
            return Err("AVIF track display scaling requires a still-image export".into());
        }
        entry_header.take(50)?;
        let mut properties =
            Properties::from_views(boxes(entry_header.data).map(|v| v.map(|v| (v, false))))?;
        if properties.extent.is_some_and(|v| v != extent) {
            return Err("Inconsistent AVIF track extent".into());
        }
        properties.extent = Some(extent);
        let mut alpha_of = None;
        let mut prem_to = None;
        if let Some(refs) = child(track, b"tref")? {
            for view in boxes(refs) {
                let view = view?;
                let target = match &view.kind {
                    b"auxl" => &mut alpha_of,
                    b"prem" => &mut prem_to,
                    _ => continue,
                };
                let mut r = Reader::new(view.data);
                if target.replace(r.u32()?).is_some() {
                    return Err("Duplicate AVIF track reference".into());
                }
                r.end()?;
            }
        }
        if alpha_of.is_some() {
            let mut r = Reader::new(required(entry_header.data, b"auxi")?);
            if r.full()? != (0, 0) || r.data != b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0" {
                return Err("Unsupported AVIF auxiliary track".into());
            }
        }
        let mut sizes = Reader::new(required(sample_table, b"stsz")?);
        if sizes.full()? != (0, 0) {
            return Err("Unsupported AVIF sample sizes".into());
        }
        let common = sizes.u32()?;
        let samples = sizes.u32()?;
        if samples == 0
            || samples > 4096
            || (common != 0 && sizes.left() != 0)
            || (common == 0 && sizes.left() != samples as usize * 4)
        {
            return Err("Invalid AVIF sample count".into());
        }
        let size = if common != 0 { common } else { sizes.u32()? };
        if size == 0 {
            return Err("Empty AVIF sample".into());
        }
        let (chunks, mut chunk_table) = if let Some(chunk) = child(sample_table, b"stco")? {
            let (n, r) = table(chunk, 4)?;
            (n, (r, false))
        } else {
            let (n, r) = table(required(sample_table, b"co64")?, 8)?;
            (n, (r, true))
        };
        if chunks == 0 {
            return Err("Missing AVIF sample chunk".into());
        }
        let offset = if chunk_table.1 {
            chunk_table.0.u64()?
        } else {
            chunk_table.0.u32()? as u64
        };
        let (count, mut mapping) = table(required(sample_table, b"stsc")?, 12)?;
        if count == 0 {
            return Err("Missing AVIF sample chunk map".into());
        }
        let mut previous = 0;
        for i in 0..count {
            let first = mapping.u32()?;
            let per_chunk = mapping.u32()?;
            let description = mapping.u32()?;
            if first <= previous
                || first > chunks
                || (i == 0 && first != 1)
                || per_chunk == 0
                || per_chunk > samples
                || description != 1
            {
                return Err("Invalid AVIF sample chunk map".into());
            }
            previous = first;
        }
        let (count, mut timing) = table(required(sample_table, b"stts")?, 8)?;
        let mut timed = 0u32;
        for _ in 0..count {
            let n = timing.u32()?;
            timing.u32()?;
            timed = timed.checked_add(n).ok_or("AVIF timing count overflow")?;
        }
        if timed != samples {
            return Err("AVIF sample timing count mismatch".into());
        }
        if let Some(sync) = child(sample_table, b"stss")? {
            let (count, mut sync) = table(sync, 4)?;
            if count == 0 || sync.u32()? != 1 {
                return Err("First AVIF sample is not independently decodable".into());
            }
        }
        let offset = usize::try_from(offset).map_err(|_| "AVIF sample offset overflow")?;
        let end = offset
            .checked_add(size as usize)
            .ok_or("AVIF sample size overflow")?;
        let sample = file
            .get(offset..end)
            .ok_or("Truncated AVIF sequence sample")?;
        Ok(Some(Self {
            id,
            alpha_of,
            prem_to,
            properties,
            sample,
            metadata: child(track, b"meta")?,
        }))
    }
}
