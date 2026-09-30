//! Shared AVIF gain-map export and previews of the delivered file.
use super::*;
use crate::photo::gainmap::{LogGain, render_pair};
use layer_core::color::hdr;

const MEMORY: &str =
    "HDR AVIF output exceeds the available memory budget. Choose a smaller export size.";

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
        check_cancel(cancel)?;
        let origin = layout.origin(index);
        let remaining = budget.checked_sub(grid_bytes(&output)).ok_or(MEMORY)?;
        let coded = encode::encode(layout.tile, color, quality, remaining, cancel, |x, y| {
            pixel(layout.index(origin, x, y))
        })?;
        output.pictures.push(coded);
    }
    Ok(output)
}

pub(in crate::photo) fn encode(
    extent: [u32; 2],
    space: RgbSpace,
    rendition: hdr::SdrRendition,
    guide: &hdr::LocalToneGuide,
    quality: u8,
    delivery: &DeliveryMetadata,
    matte: Option<[f32; 3]>,
    clip: bool,
    budget: usize,
    cancel: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<(Vec<u8>, crate::OutputStatistics), String> {
    let layout = Layout::new(extent)?;
    let count = admit(layout, budget)?;
    let mut master = buffer::<[f32; 3]>(count)?;
    let mut base = buffer::<[u16; 4]>(count)?;
    let mut transparent = false;
    let (stats, peak) = render_pair(
        extent, space, rendition, guide, quality, matte, clip, cancel, read,
        |hdr, sdr, a| {
            let alpha = if matte.is_some() {
                4095
            } else {
                (a * 4095.).round() as u16
            };
            transparent |= alpha < 4095;
            let [r, g, b] = sdr.map(|v| {
                (RgbSpace::Srgb.encode(f64::from(v)).clamp(0., 1.) * 4095.).round() as u16
            });
            master.push(hdr);
            base.push([r, g, b, alpha]);
            Ok(())
        },
    )?;
    // Reserve container metadata and row/guide scratch independently of each
    // packet's actual retained capacity. Compressed sizes are checked at every
    // cell, including before assembling the second copy in the final file.
    let budget = budget.checked_sub(8 * 1024 * 1024).ok_or(MEMORY)?;
    let retained = master.capacity() * 12 + base.capacity() * 8;
    let base_grid = grid(
        layout,
        Some(mux::BASE),
        quality,
        budget.checked_sub(retained).ok_or(MEMORY)?,
        cancel,
        |i| {
            let [r, g, b, _] = base[i];
            [g, b, r]
        },
    )?;
    let mut gains = LogGain::default();
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
            check_cancel(cancel)?;
            for x in 0..layout.tile[0].min(extent[0] - origin[0]) {
                let logs = &mut master[layout.index(origin, x, y)];
                for (c, plane_index) in [2, 0, 1].into_iter().enumerate() {
                    // Match the reader's normalized 16-bit RGB handoff to ICC.
                    let code = ((u32::from(plane.sample(plane_index, x, y)) * 65535 + 2047) / 4095)
                        as f64
                        / 65535.;
                    gains.apply(&mut logs[c], code);
                }
            }
        }
    }
    let metadata = gains.metadata(peak);
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
        Some(mux::GAIN),
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
        delivery,
        budget.checked_sub(retained).ok_or(MEMORY)?,
        cancel,
    )?;
    check_cancel(cancel)?;
    Ok((bytes, stats))
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
    let mut rows = source.rows();
    crate::preview_encoded_rows(source.extent, bounds, RgbSpace::Srgb, &source.interpretation, |y, row| {
        check_cancel(cancel)?;
        rows.read(y, row)
    })
}

pub(in crate::photo) fn preview(
    extent: [u32; 2],
    bounds: [u32; 2],
    space: RgbSpace,
    rendition: hdr::SdrRendition,
    guide: &hdr::LocalToneGuide,
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
    let (bytes, stats) = encode(
        extent,
        space,
        rendition,
        guide,
        options.quality,
        &Default::default(),
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
