//! Bounded AVIF still/grid writer with alpha and an ISO tone-map alternative.
use super::*;
use crate::photo::GainMapMetadata;

pub(super) struct Grid {
    pub extent: [u32; 2],
    pub tile_extent: [u32; 2],
    pub columns: u32,
    pub rows: u32,
    pub pictures: Vec<encode::Coded>,
}

struct Writer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Writer {
    fn new(limit: usize) -> Result<Self, String> {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(limit).map_err(err)?;
        Ok(Self { bytes, limit })
    }
    fn put(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > self.limit)
        {
            return Err("AVIF container exceeds its admitted size".into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn u8(&mut self, n: u8) -> Result<(), String> {
        self.put(&[n])
    }
    fn u16(&mut self, n: u16) -> Result<(), String> {
        self.put(&n.to_be_bytes())
    }
    fn u32(&mut self, n: u32) -> Result<(), String> {
        self.put(&n.to_be_bytes())
    }
    fn boxed(
        &mut self,
        kind: &[u8; 4],
        write: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        let at = self.bytes.len();
        self.u32(0)?;
        self.put(kind)?;
        write(self)?;
        let size = u32::try_from(self.bytes.len() - at).map_err(|_| "AVIF box size overflow")?;
        self.bytes[at..at + 4].copy_from_slice(&size.to_be_bytes());
        Ok(())
    }
}

struct Item<'a> {
    kind: [u8; 4],
    hidden: bool,
    data: Cow<'a, [u8]>,
    properties: Vec<u8>,
}
struct Reference {
    kind: [u8; 4],
    from: u16,
    to: Vec<u16>,
}
struct Builder<'a> {
    items: Vec<Item<'a>>,
    properties: Vec<Vec<u8>>,
    references: Vec<Reference>,
}
impl<'a> Builder<'a> {
    fn item(&mut self, kind: &[u8; 4], hidden: bool, data: Cow<'a, [u8]>) -> Result<u16, String> {
        if self.items.len() == 4096 {
            return Err("Too many AVIF output items".into());
        }
        self.items.push(Item {
            kind: *kind,
            hidden,
            data,
            properties: Vec::new(),
        });
        Ok(self.items.len() as u16)
    }
    fn property(
        &mut self,
        id: u16,
        kind: &[u8; 4],
        body: &[u8],
        essential: bool,
    ) -> Result<(), String> {
        let mut w = Writer::new(body.len() + 8)?;
        w.boxed(kind, |w| w.put(body))?;
        let index = match self.properties.iter().position(|p| *p == w.bytes) {
            Some(i) => i,
            None => {
                self.properties.push(w.bytes);
                self.properties.len() - 1
            }
        };
        if index >= 127 {
            return Err("Too many AVIF output properties".into());
        }
        self.items[id as usize - 1]
            .properties
            .push(index as u8 + 1 | if essential { 128 } else { 0 });
        Ok(())
    }
    fn image_properties(
        &mut self,
        id: u16,
        extent: [u32; 2],
        channels: u8,
        depth: u8,
        color: Option<Color>,
        config: Option<&[u8]>,
    ) -> Result<(), String> {
        let mut ispe = vec![0; 4];
        ispe.extend(extent.into_iter().flat_map(u32::to_be_bytes));
        self.property(id, b"ispe", &ispe, false)?;
        let mut pixi = vec![0, 0, 0, 0, channels];
        pixi.extend(std::iter::repeat_n(depth, channels as usize));
        self.property(id, b"pixi", &pixi, false)?;
        if let Some(config) = config {
            self.property(id, b"av1C", config, true)?;
        }
        if let Some(color) = color {
            let mut nclx = b"nclx".to_vec();
            nclx.extend(color.cicp.into_iter().flat_map(u16::to_be_bytes));
            nclx.push(if color.full_range { 128 } else { 0 });
            self.property(id, b"colr", &nclx, false)?;
        }
        Ok(())
    }
    fn grid(
        &mut self,
        grid: &'a Grid,
        color: Option<Color>,
        hidden: bool,
        cancel: &AtomicBool,
    ) -> Result<u16, String> {
        if grid.rows == 0
            || grid.columns == 0
            || grid.rows > 256
            || grid.columns > 256
            || grid.pictures.len() != (grid.rows * grid.columns) as usize
            || grid.pictures.iter().any(|p| p.extent != grid.tile_extent)
            || [grid.columns, grid.rows]
                .into_iter()
                .enumerate()
                .any(|(i, n)| {
                    u64::from(grid.tile_extent[i]) * u64::from(n) < u64::from(grid.extent[i])
                        || u64::from(grid.tile_extent[i]) * u64::from(n - 1)
                            >= u64::from(grid.extent[i])
                })
        {
            return Err("Invalid AVIF encoder grid".into());
        }
        let channels = if color.is_some() { 3 } else { 1 };
        if grid.pictures.len() == 1 && grid.extent == grid.tile_extent {
            let p = &grid.pictures[0];
            let id = self.item(b"av01", hidden, Cow::Borrowed(&p.bytes))?;
            self.image_properties(id, grid.extent, channels, 12, color, Some(&p.config))?;
            return Ok(id);
        }
        let mut data = vec![0, 1, (grid.rows - 1) as u8, (grid.columns - 1) as u8];
        data.extend(grid.extent.into_iter().flat_map(u32::to_be_bytes));
        let root = self.item(b"grid", hidden, Cow::Owned(data))?;
        self.image_properties(root, grid.extent, channels, 12, color, None)?;
        let mut tiles = Vec::new();
        for p in &grid.pictures {
            codec::check(cancel)?;
            let id = self.item(b"av01", true, Cow::Borrowed(&p.bytes))?;
            self.image_properties(id, p.extent, channels, 12, color, Some(&p.config))?;
            tiles.push(id);
        }
        self.references.push(Reference {
            kind: *b"dimg",
            from: root,
            to: tiles,
        });
        Ok(root)
    }
}

fn tone_map(metadata: GainMapMetadata) -> Result<Vec<u8>, String> {
    if !metadata.min_log2.is_finite()
        || !metadata.max_log2.is_finite()
        || !metadata.offset.is_finite()
        || !metadata.headroom.is_finite()
        || metadata.min_log2 < -64.
        || metadata.max_log2 > 64.
        || metadata.min_log2 >= metadata.max_log2
        || !(0. ..=1.).contains(&metadata.offset)
        || !(0. ..=64.).contains(&metadata.headroom)
        || metadata.headroom == 0.
    {
        return Err("Invalid AVIF output gain-map metadata".into());
    }
    let mut w = Writer::new(62)?;
    w.u8(0)?; // ToneMappedImageBox version
    w.u16(0)?; // minimum metadata version
    w.u16(0)?; // writer version
    w.u8(0x40)?; // single-channel metadata, use base color space
    w.u32(0)?;
    w.u32(1)?; // base headroom
    w.u32((metadata.headroom * 1_000_000.).round() as u32)?;
    w.u32(1_000_000)?;
    for n in [metadata.min_log2, metadata.max_log2] {
        w.u32((n * 1_000_000.).round() as i32 as u32)?;
        w.u32(1_000_000)?;
    }
    w.u32(1)?;
    w.u32(1)?; // gamma
    for _ in 0..2 {
        w.u32((metadata.offset * 1_000_000.).round() as u32)?;
        w.u32(1_000_000)?;
    }
    Ok(w.bytes)
}

pub(super) fn assemble(
    base: &Grid,
    alpha: Option<&Grid>,
    gain: &Grid,
    metadata: GainMapMetadata,
    resolution: Option<layer_core::ImageResolution>,
    budget: usize,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if gain.extent != base.extent || alpha.is_some_and(|a| a.extent != base.extent) {
        return Err("AVIF output planes disagree in extent".into());
    }
    let mut b = Builder {
        items: Vec::new(),
        properties: Vec::new(),
        references: Vec::new(),
    };
    let base_id = b.grid(
        base,
        Some(Color {
            cicp: [9, 13, 0],
            full_range: true,
        }),
        false,
        cancel,
    )?;
    if let Some(alpha) = alpha {
        let alpha_id = b.grid(alpha, None, true, cancel)?;
        let mut aux = vec![0; 4];
        aux.extend_from_slice(b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0");
        b.property(alpha_id, b"auxC", &aux, true)?;
        b.references.push(Reference {
            kind: *b"auxl",
            from: alpha_id,
            to: vec![base_id],
        });
    }
    let gain_id = b.grid(
        gain,
        Some(Color {
            cicp: [2, 2, 0],
            full_range: true,
        }),
        true,
        cancel,
    )?;
    let tmap_id = b.item(b"tmap", false, Cow::Owned(tone_map(metadata)?))?;
    b.image_properties(
        tmap_id,
        base.extent,
        if alpha.is_some() { 4 } else { 3 },
        16,
        Some(Color {
            cicp: [9, 8, 0],
            full_range: true,
        }),
        None,
    )?;
    b.references.push(Reference {
        kind: *b"dimg",
        from: tmap_id,
        to: vec![base_id, gain_id],
    });
    if let Some(resolution) = resolution {
        let exif = super::super::metadata::exif_output(resolution)?;
        let mut data = vec![0; 4];
        data.extend_from_slice(&exif[6..]);
        let id = b.item(b"Exif", true, Cow::Owned(data))?;
        // The tone-map derives its source metadata from the base. A single
        // content-description target also works with readers that store only
        // one metadata association per item (including libavif).
        b.references.push(Reference {
            kind: *b"cdsc",
            from: id,
            to: vec![base_id],
        });
    }
    let mut ftyp = Writer::new(32)?;
    ftyp.boxed(b"ftyp", |w| {
        w.put(b"avif")?;
        w.u32(0)?;
        w.put(b"avifmif1miaftmap")
    })?;
    let mut meta = Writer::new(budget.min(1024 * 1024))?;
    let mut offsets = Vec::new();
    meta.boxed(b"meta", |w| {
        w.u32(0)?;
        w.boxed(b"hdlr", |w| {
            w.u32(0)?;
            w.u32(0)?;
            w.put(b"pict")?;
            w.put(&[0; 12])?;
            w.u8(0)
        })?;
        w.boxed(b"pitm", |w| {
            w.u32(0)?;
            w.u16(base_id)
        })?;
        w.boxed(b"iinf", |w| {
            w.u32(0)?;
            w.u16(b.items.len() as u16)?;
            for (index, item) in b.items.iter().enumerate() {
                codec::check(cancel)?;
                w.boxed(b"infe", |w| {
                    w.u32((2 << 24) | u32::from(item.hidden))?;
                    w.u16(index as u16 + 1)?;
                    w.u16(0)?;
                    w.put(&item.kind)?;
                    w.u8(0)
                })?;
            }
            Ok(())
        })?;
        w.boxed(b"iloc", |w| {
            w.u32(0)?;
            w.u8(0x44)?;
            w.u8(0)?;
            w.u16(b.items.len() as u16)?;
            for (index, item) in b.items.iter().enumerate() {
                w.u16(index as u16 + 1)?;
                w.u16(0)?;
                w.u16(1)?;
                offsets.push(w.bytes.len());
                w.u32(0)?;
                w.u32(u32::try_from(item.data.len()).map_err(|_| "AVIF payload too large")?)?;
            }
            Ok(())
        })?;
        w.boxed(b"iprp", |w| {
            w.boxed(b"ipco", |w| {
                for p in &b.properties {
                    w.put(p)?;
                }
                Ok(())
            })?;
            w.boxed(b"ipma", |w| {
                w.u32(0)?;
                w.u32(b.items.len() as u32)?;
                for (index, item) in b.items.iter().enumerate() {
                    w.u16(index as u16 + 1)?;
                    w.u8(item.properties.len() as u8)?;
                    w.put(&item.properties)?;
                }
                Ok(())
            })
        })?;
        w.boxed(b"iref", |w| {
            w.u32(0)?;
            for r in &b.references {
                w.boxed(&r.kind, |w| {
                    w.u16(r.from)?;
                    w.u16(r.to.len() as u16)?;
                    for &id in &r.to {
                        w.u16(id)?;
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })?;
        w.boxed(b"grpl", |w| {
            w.boxed(b"altr", |w| {
                w.u32(0)?;
                w.u32(b.items.len() as u32 + 1)?;
                w.u32(2)?;
                w.u32(tmap_id.into())?;
                w.u32(base_id.into())
            })
        })
    })?;
    let mut at = ftyp.bytes.len() + meta.bytes.len() + 8;
    for (offset, item) in offsets.into_iter().zip(&b.items) {
        let start = u32::try_from(at).map_err(|_| "AVIF file exceeds 4 GiB")?;
        meta.bytes[offset..offset + 4].copy_from_slice(&start.to_be_bytes());
        at = at
            .checked_add(item.data.len())
            .ok_or("AVIF file size overflow")?;
    }
    if at > budget || at > u32::MAX as usize {
        return Err("AVIF output exceeds its memory budget".into());
    }
    let mut output = Writer::new(at)?;
    output.put(&ftyp.bytes)?;
    output.put(&meta.bytes)?;
    output.boxed(b"mdat", |w| {
        for item in &b.items {
            codec::check(cancel)?;
            w.put(&item.data)?;
        }
        Ok(())
    })?;
    Ok(output.bytes)
}
