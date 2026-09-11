use crate::ReadbackImage;
use std::io::Write;

impl ReadbackImage {
    /// Explicit export of packed, straight-alpha, display-encoded sRGB pixels.
    /// No viewport, cursor, selection outline or native chrome enters this image.
    pub fn write_png(&self, output: impl Write) -> Result<(), String> {
        let stride = self.width.checked_mul(4).ok_or("Invalid canvas export")?;
        let size = (stride as usize)
            .checked_mul(self.height as usize)
            .ok_or("Invalid canvas export")?;
        if self.width == 0 || self.height == 0 || self.stride != stride || self.bytes.len() != size
        {
            return Err("Invalid canvas export".into());
        }
        let mut encoder = png::Encoder::new(output, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&self.bytes)
            .map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_preserves_dimensions_srgb_and_straight_alpha_bytes() {
        let image = ReadbackImage {
            request_id: 7,
            width: 3,
            height: 2,
            stride: 12,
            bytes: vec![
                0, 1, 255, 0, 15, 127, 230, 60, 255, 0, 95, 128, 33, 2, 13, 255, 255, 255, 255,
                255, 0, 0, 0, 1,
            ],
        };
        let mut encoded = Vec::new();
        image.write_png(&mut encoded).unwrap();
        let mut reader = png::Decoder::new(encoded.as_slice()).read_info().unwrap();
        assert_eq!(
            reader.info().srgb,
            Some(png::SrgbRenderingIntent::Perceptual)
        );
        let mut decoded = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut decoded).unwrap();
        assert_eq!([info.width, info.height], [3, 2]);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(decoded, image.bytes);
        let mut bad = image;
        bad.stride = 16;
        assert!(bad.write_png(Vec::new()).is_err());
    }
}
