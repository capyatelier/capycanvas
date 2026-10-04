use super::*;
impl SnapshotRenderer {
    /// Full native composition converted into a separate editable raster master.
    pub fn flattened_document(
        &mut self,
        color: layer_core::color::DocumentColor,
        options: layer_core::color::ConversionOptions,
        limit: usize,
    ) -> Result<layer_color::PreparedDocumentColor, String> {
        if self.output_extent != self.extent {
            return Err("A converted copy must retain the original canvas extent".into());
        }
        let resolution = self.output_metadata.resolution;
        let metadata = self.output_metadata.photo.clone();
        let target = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: color.depth,
            profile: layer_core::color::ColorProfile::Builtin(color.space),
            profile_assumed: false,
        };
        let mut document = None;
        let statistics = self.write_rows(
            &target,
            layer_core::color::OutputEncoding {
                conversion: options,
                ..Default::default()
            },
            None,
            |extent, actual, read| {
                document = Some(layer_color::flattened_document(
                    extent, color, resolution, actual, limit, read,
                )?);
                Ok(())
            },
        )?;
        let mut document = document.ok_or("The converted copy is incomplete")?;
        document.artwork.metadata = Arc::new(metadata);
        Ok(layer_color::PreparedDocumentColor {
            document,
            statistics,
        })
    }
}
