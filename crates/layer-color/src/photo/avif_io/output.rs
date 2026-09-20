//! Shared AVIF gain-map export and previews of the delivered file.
use super::*;
use crate::photo::GainMapMetadata;
use layer_core::color::{hdr, rgb};
use std::sync::atomic::Ordering;

const MEMORY: &str =
    "HDR AVIF output exceeds the available memory budget. Choose a smaller export size.";
const OFFSET: f32 = 1. / 64.;
const BASE: Color = Color {
    cicp: [9, 13, 0],
    full_range: true,
};
const GAIN: Color = Color {
    cicp: [2, 2, 0],
    full_range: true,
};

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
struct Layout {
    extent: [u32; 2],
    tile: [u32; 2],
    columns: u32,
    rows: u32,
}
impl Layout {
    fn new(extent: [u32; 2]) -> Result<Self, String> {
        validate_extent(extent, 32768)?;
        // Keep individual synchronous encoder calls small. Larger documents
        // use larger cells to stay inside the reader's 4096-item limit, even
        // with both alpha and gain maps. Edge cells repeat the last sample;
        // the grid's spatial extent crops their 8-pixel alignment padding.
        let side = [256, 512, 1024]
            .into_iter()
            .find(|side| {
                u64::from(extent[0].div_ceil(*side)) * u64::from(extent[1].div_ceil(*side)) * 3 + 5
                    <= 4096
            })
            .ok_or("AVIF output requires too many grid cells")?;
        let needs_grid = extent.iter().any(|n| *n > side || n % 8 != 0);
        // MIAF requires both coded tile dimensions to be at least 64, even
        // for a one-cell grid used only to trim alignment padding.
        // Balance cells so a short final strip does not encode a mostly padded
        // full-size cell in every row/column. Keep the same maximum cell size.
        let tile = extent.map(|n| {
            let cells = n.div_ceil(side);
            (n.div_ceil(cells).div_ceil(8) * 8).max(if needs_grid { 64 } else { 8 })
        });
        Ok(Self {
            extent,
            tile,
            columns: extent[0].div_ceil(tile[0]),
            rows: extent[1].div_ceil(tile[1]),
        })
    }
    fn origin(self, index: usize) -> [u32; 2] {
        [
            (index as u32 % self.columns) * self.tile[0],
            (index as u32 / self.columns) * self.tile[1],
        ]
    }
    fn index(self, origin: [u32; 2], x: u32, y: u32) -> usize {
        (origin[1] + y).min(self.extent[1] - 1) as usize * self.extent[0] as usize
            + (origin[0] + x).min(self.extent[0] - 1) as usize
    }
}

fn admit(layout: Layout, budget: usize) -> Result<usize, String> {
    let count = u64::from(layout.extent[0]) * u64::from(layout.extent[1]);
    let cell = layout.tile.map(|n| n.div_ceil(64) * 64);
    // Master/log floats, SDR/alpha samples, retained AV1 packets, container
    // copy, and one encoder working set. Use u64 before converting for Wasm.
    let needed = count * 96 + u64::from(cell[0]) * u64::from(cell[1]) * 192 + 24 * 1024 * 1024;
    if needed > budget as u64 {
        return Err(MEMORY.into());
    }
    usize::try_from(count).map_err(|_| MEMORY.into())
}
fn buffer<T>(count: usize) -> Result<Vec<T>, String> {
    let mut v = Vec::new();
    v.try_reserve_exact(count).map_err(|_| MEMORY)?;
    Ok(v)
}
fn grid_bytes(grid: &mux::Grid) -> usize {
    grid.pictures.capacity() * std::mem::size_of::<encode::Coded>()
        + grid
            .pictures
            .iter()
            .map(|p| p.bytes.capacity() + p.config.capacity())
            .sum::<usize>()
}
fn grid(
    layout: Layout,
    color: Option<Color>,
    quality: u8,
    budget: usize,
    cancel: &AtomicBool,
    pixel: impl Fn(usize) -> [u16; 3],
) -> Result<mux::Grid, String> {
    let mut output = mux::Grid {
        extent: layout.extent,
        tile_extent: layout.tile,
        columns: layout.columns,
        rows: layout.rows,
        pictures: buffer((layout.columns * layout.rows) as usize)?,
    };
    for index in 0..layout.columns as usize * layout.rows as usize {
        codec::check(cancel)?;
        let origin = layout.origin(index);
        let remaining = budget.checked_sub(grid_bytes(&output)).ok_or(MEMORY)?;
        let coded = encode::encode(layout.tile, color, quality, remaining, cancel, |x, y| {
            pixel(layout.index(origin, x, y))
        })?;
        output.pictures.push(coded);
    }
    Ok(output)
}

fn encode(
    extent: [u32; 2],
    space: RgbSpace,
    rendition: hdr::SdrRendition,
    guide: Option<&hdr::LocalToneGuide>,
    quality: u8,
    resolution: Option<layer_core::ImageResolution>,
    matte: Option<[f32; 3]>,
    clip: bool,
    budget: usize,
    cancel: &AtomicBool,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<(Vec<u8>, crate::OutputStatistics), String> {
    codec::check(cancel)?;
    rendition.validate().map_err(str::to_string)?;
    if !(1..=100).contains(&quality) {
        return Err("Invalid HDR quality".into());
    }
    if matte.is_some_and(|p| p.iter().any(|v| !v.is_finite() || !(0. ..=1.).contains(v))) {
        return Err("Invalid HDR background".into());
    }
    let layout = Layout::new(extent)?;
    let count = admit(layout, budget)?;
    let generated = if guide.is_none() {
        Some(crate::build_local_tone_guide(
            extent,
            space,
            || cancel.load(Ordering::Relaxed),
            &mut read,
        )?)
    } else {
        None
    };
    let guide = guide
        .or(generated.as_ref())
        .ok_or("Missing HDR tone guide")?;
    let matrix = hdr::to_bt2020(space);
    let mapper = rendition.mapper(space, RgbSpace::Srgb);
    let mut master = buffer::<[f32; 3]>(count)?;
    let mut base = buffer::<[u16; 4]>(count)?;
    let mut row = vec![[0.; 4]; extent[0] as usize];
    let mut stats = crate::OutputStatistics::default();
    let mut peak = 1f32;
    let mut transparent = false;
    for y in 0..extent[1] {
        codec::check(cancel)?;
        read(y, &mut row)?;
        for (x, p) in row.iter().enumerate() {
            if p.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&p[3]) {
                return Err("Invalid HDR output pixel".into());
            }
            let a = p[3];
            let raw = std::array::from_fn(|c| if a > 0. { f64::from(p[c] / a) } else { 0. });
            let mut values = rgb::apply(matrix, raw).map(|v| v as f32);
            let position = [
                (x as f32 + 0.5) * guide.document_extent[0] as f32 / extent[0] as f32,
                (y as f32 + 0.5) * guide.document_extent[1] as f32 / extent[1] as f32,
            ];
            let mapped = mapper.map_local_premultiplied(*p, position, guide);
            let mut sdr = rgb::apply(
                hdr::srgb_to_bt2020(),
                std::array::from_fn(|c| if a > 0. { f64::from(mapped[c] / a) } else { 0. }),
            )
            .map(|v| v as f32);
            let mut codes = [0; 4];
            codes[3] = if matte.is_some() {
                4095
            } else {
                (a * 4095.).round() as u16
            };
            transparent |= codes[3] < 4095;
            for c in 0..3 {
                sdr[c] = sdr[c].clamp(0., 1.);
                if let Some(background) = matte {
                    values[c] = values[c] * a + background[c] * (1. - a);
                    sdr[c] = sdr[c] * a + background[c] * (1. - a);
                }
                if !values[c].is_finite() {
                    return Err("Invalid HDR output pixel".into());
                }
                if values[c] < -1e-6 || values[c] > hdr::MAX_LINEAR {
                    if !clip {
                        return Err("HDR gain-map output exceeds BT.2020 or the half-float range. Enable Clip out-of-range colors to export a mapped copy.".into());
                    }
                    stats.clipped_channels += 1;
                }
                values[c] = values[c].clamp(0., hdr::MAX_LINEAR);
                peak = peak.max(values[c]);
                codes[c] =
                    (RgbSpace::Srgb.encode(f64::from(sdr[c])).clamp(0., 1.) * 4095.).round() as u16;
            }
            master.push(values);
            base.push(codes);
        }
    }
    drop(row);
    // Reserve container metadata and row/guide scratch independently of each
    // packet's actual retained capacity. Compressed sizes are checked at every
    // cell, including before assembling the second copy in the final file.
    let budget = budget.checked_sub(8 * 1024 * 1024).ok_or(MEMORY)?;
    let retained = master.capacity() * 12 + base.capacity() * 8;
    let base_grid = grid(
        layout,
        Some(BASE),
        quality,
        budget.checked_sub(retained).ok_or(MEMORY)?,
        cancel,
        |i| {
            let [r, g, b, _] = base[i];
            [g, b, r]
        },
    )?;
    let mut low = 0f32;
    let mut high = 0f32;
    for (index, coded) in base_grid.pictures.iter().enumerate() {
        let origin = layout.origin(index);
        let plane = codec::decode(
            &coded.bytes,
            &coded.config[4..],
            coded.extent,
            budget
                .checked_sub(retained + grid_bytes(&base_grid))
                .ok_or(MEMORY)?,
            cancel,
        )?;
        if plane.depth != 12 || plane.layout != 3 {
            return Err("Unexpected encoded AVIF base format".into());
        }
        for y in 0..layout.tile[1].min(extent[1] - origin[1]) {
            codec::check(cancel)?;
            for x in 0..layout.tile[0].min(extent[0] - origin[0]) {
                let logs = &mut master[layout.index(origin, x, y)];
                for (c, plane_index) in [2, 0, 1].into_iter().enumerate() {
                    // Match the reader's normalized 16-bit RGB handoff to ICC.
                    let code = ((u32::from(plane.sample(plane_index, x, y)) * 65535 + 2047) / 4095)
                        as f64
                        / 65535.;
                    logs[c] =
                        ((logs[c] + OFFSET) / (RgbSpace::Srgb.decode(code) as f32 + OFFSET)).log2();
                    low = low.min(logs[c]);
                    high = high.max(logs[c]);
                }
            }
        }
    }
    let metadata = GainMapMetadata {
        min_log2: low,
        max_log2: high.max(low + 0.001),
        offset: OFFSET,
        headroom: peak.log2().max(0.001),
    };
    let alpha_grid = if transparent {
        Some(grid(
            layout,
            None,
            100,
            budget
                .checked_sub(retained + grid_bytes(&base_grid))
                .ok_or(MEMORY)?,
            cancel,
            |i| [base[i][3]; 3],
        )?)
    } else {
        None
    };
    drop(base);
    let retained =
        master.capacity() * 12 + grid_bytes(&base_grid) + alpha_grid.as_ref().map_or(0, grid_bytes);
    // Log-gain errors multiply into HDR brightness. Give the gain map a smaller
    // quality deficit than the SDR base, using the same delivery control.
    // Every quality below 100 stays lossy; 100 preserves the 12-bit samples.
    let gain_grid = grid(
        layout,
        Some(GAIN),
        100 - (100 - quality).div_ceil(4),
        budget.checked_sub(retained).ok_or(MEMORY)?,
        cancel,
        |i| {
            let [r, g, b] =
                master[i].map(|v| (metadata.encode(v).clamp(0., 1.) * 4095.).round() as u16);
            [g, b, r]
        },
    )?;
    drop(master);
    let retained =
        grid_bytes(&base_grid) + grid_bytes(&gain_grid) + alpha_grid.as_ref().map_or(0, grid_bytes);
    let bytes = mux::assemble(
        &base_grid,
        alpha_grid.as_ref(),
        &gain_grid,
        metadata,
        resolution,
        budget.checked_sub(retained).ok_or(MEMORY)?,
        cancel,
    )?;
    codec::check(cancel)?;
    Ok((bytes, stats))
}

pub(in crate::photo) fn write(
    mut output: impl Write,
    extent: [u32; 2],
    space: RgbSpace,
    rendition: hdr::SdrRendition,
    guide: Option<&hdr::LocalToneGuide>,
    options: impl Into<GainMapEncodeOptions>,
    resolution: Option<layer_core::ImageResolution>,
    matte: Option<[f32; 3]>,
    clip: bool,
    cancel: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<crate::OutputStatistics, String> {
    let options = options.into();
    let (bytes, stats) = encode(
        extent,
        space,
        rendition,
        guide,
        options.quality,
        resolution,
        matte,
        clip,
        options.memory.encode_bytes,
        cancel,
        read,
    )?;
    for chunk in bytes.chunks(65536) {
        codec::check(cancel)?;
        output.write_all(chunk).map_err(err)?;
    }
    codec::check(cancel)?;
    output.flush().map_err(err)?;
    Ok(stats)
}

fn preview_rendition(
    bytes: &[u8],
    bounds: [u32; 2],
    hdr: bool,
    retained: usize,
    budget: PhotoMemoryBudget,
    cancel: &AtomicBool,
) -> Result<([u32; 2], Vec<[f32; 4]>), String> {
    let mut limits = DecodeLimits::from_memory_budget(budget);
    // Both AreaPreview's f64 accumulation and its finished f32 output can
    // coexist, as can the previously generated rendition and encoded file.
    let scratch = u64::from(bounds[0]) * u64::from(bounds[1]) * 64 + 4 * 1024 * 1024;
    limits.codec_bytes = limits
        .codec_bytes
        .checked_sub(retained)
        .and_then(|n| n.checked_sub(usize::try_from(scratch).ok()?))
        .ok_or(MEMORY)?;
    let source = read_rendition(std::io::Cursor::new(bytes), limits, cancel, hdr)?.source;
    let decoder =
        crate::WorkingDecoder::new(&source.interpretation, RgbSpace::Srgb, Default::default())?;
    let mut preview = crate::AreaPreview::new(source.extent, bounds)?;
    let mut row = vec![0; source.row_bytes()];
    let mut pixels = vec![[0.; 4]; source.extent[0] as usize];
    let mut rows = source.rows();
    for y in 0..source.extent[1] {
        codec::check(cancel)?;
        rows.read(y, &mut row)?;
        decoder.decode_pixels(&row, &mut pixels)?;
        for p in &mut pixels {
            for c in 0..3 {
                p[c] *= p[3];
            }
        }
        preview.push(&pixels)?;
    }
    preview.finish()
}

pub(in crate::photo) fn preview(
    extent: [u32; 2],
    bounds: [u32; 2],
    space: RgbSpace,
    rendition: hdr::SdrRendition,
    guide: Option<&hdr::LocalToneGuide>,
    options: impl Into<GainMapEncodeOptions>,
    matte: Option<[f32; 3]>,
    cancel: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<
    (
        [u32; 2],
        Vec<[f32; 4]>,
        Vec<[f32; 4]>,
        crate::OutputStatistics,
    ),
    String,
> {
    let options = options.into();
    if bounds.into_iter().any(|n| !(1..=1024).contains(&n)) {
        return Err("Invalid preview dimensions".into());
    }
    let (bytes, stats) = encode(
        extent,
        space,
        rendition,
        guide,
        options.quality,
        None,
        matte,
        true,
        options.memory.encode_bytes,
        cancel,
        read,
    )?;
    let (preview_extent, hdr) = preview_rendition(&bytes, bounds, true, bytes.capacity(), options.memory, cancel)?;
    let (_, sdr) = preview_rendition(
        &bytes,
        bounds,
        false,
        bytes.capacity() + hdr.capacity() * 16,
        options.memory,
        cancel,
    )?;
    Ok((preview_extent, hdr, sdr, stats))
}
