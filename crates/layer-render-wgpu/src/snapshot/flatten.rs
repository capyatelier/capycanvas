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
        let resolution = self.output_resolution;
        let target = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: color.depth,
            profile: layer_core::color::ColorProfile::Builtin(color.space),
            profile_assumed: false,
        };
        let mut project = None;
        let statistics = self.write_rows(
            &target,
            layer_core::color::OutputEncoding {
                conversion: options,
                ..Default::default()
            },
            None,
            |extent, actual, read| {
                project = Some(layer_color::flattened_document(
                    extent, color, resolution, actual, limit, read,
                )?);
                Ok(())
            },
        )?;
        let project = project.ok_or("The converted copy is incomplete")?;
        let allocated_bytes = project.document.layers[0]
            .source
            .as_ref()
            .unwrap()
            .resident_bytes();
        Ok(layer_color::PreparedDocumentColor {
            project,
            statistics,
            allocated_bytes,
        })
    }
}
