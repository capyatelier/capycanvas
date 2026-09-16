//! One bounded delivery pipeline for GPU snapshot workers and browser file
//! workers. The reader supplies exact linear-premultiplied working pixels.
use crate::{OutputStatistics, RowResampler, WorkingEncoder};
use layer_core::color::{OutputEncoding, RgbSpace, source::SourceInterpretation};

pub fn encode_working_rows(
    working: RgbSpace,
    source_extent: [u32; 2],
    extent: [u32; 2],
    target: &SourceInterpretation,
    options: OutputEncoding,
    matte: Option<[f32; 3]>,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
    write: impl FnOnce(
        [u32; 2],
        &SourceInterpretation,
        &mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>,
    ) -> Result<(), String>,
) -> Result<OutputStatistics, String> {
    let encoder = WorkingEncoder::new(working, target, options)?;
    let mut resampler = (source_extent != extent)
        .then(|| RowResampler::new(source_extent, extent))
        .transpose()?;
    let mut pixels = vec![[0.; 4]; extent[0] as usize];
    let mut statistics = OutputStatistics::default();
    write(extent, encoder.interpretation(), &mut |y, row| {
        if let Some(resampler) = &mut resampler {
            resampler.read_row(y, &mut pixels, &mut read)?;
        } else {
            read(y, &mut pixels)?;
        }
        statistics.clipped_channels += encoder
            .encode_premultiplied(&pixels, row, matte, [0, y])?
            .clipped_channels;
        Ok(())
    })?;
    Ok(statistics)
}
