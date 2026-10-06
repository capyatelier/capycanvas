use super::*;
use layer_core::sample_remap::{GridMap, remap_image};

/// Normalize the source's sample positions losslessly, one bounded tile at a time.
pub(super) fn normalize(
    mut source: SourceImage,
    resolution: Option<layer_core::ImageResolution>,
    orientation: u16,
    max_bytes: usize,
) -> Result<SourceImage, String> {
    source.resolution = resolution;
    if orientation == 1 {
        return Ok(source);
    }
    let (map, extent) = GridMap::exif(orientation, source.extent).ok_or("Invalid source orientation")?;
    remap_image(&source, extent, map, max_bytes, &Default::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photo::test_support::rows;
    #[test]
    fn all_eight_orientations_preserve_numbered_pixel_positions() {
        let interpretation = SourceInterpretation {
            channels: SourceChannels::Gray,
            depth: SampleDepth::U8,
            profile: ColorProfile::default(),
            profile_assumed: true,
        };
        let mut builder = SourceBuilder::new([3, 2], interpretation, 1024 * 1024).unwrap();
        builder.push_row(&[1, 2, 3]).unwrap();
        builder.push_row(&[4, 5, 6]).unwrap();
        let source = builder.finish().unwrap();
        let expected = [
            [1, 2, 3, 4, 5, 6],
            [3, 2, 1, 6, 5, 4],
            [6, 5, 4, 3, 2, 1],
            [4, 5, 6, 1, 2, 3],
            [1, 4, 2, 5, 3, 6],
            [4, 1, 5, 2, 6, 3],
            [6, 3, 5, 2, 4, 1],
            [3, 6, 2, 5, 1, 4],
        ];
        for orientation in 1..=8 {
            let result =
                normalize(source.clone(), source.resolution, orientation, 1024 * 1024).unwrap();
            assert_eq!(rows(&result).concat(), expected[orientation as usize - 1]);
        }
    }
}
