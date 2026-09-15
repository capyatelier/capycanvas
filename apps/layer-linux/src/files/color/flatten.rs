//! Stream the complete composition into a separate editable raster document.
use super::*;
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation, SourceKind};
use layer_render_wgpu::snapshot::SnapshotGpu;

pub(super) fn prepare(
    gpu: SnapshotGpu,
    project: Project,
    color: DocumentColor,
    options: ConversionOptions,
    background: [f32; 4],
    time: f32,
    control: CaptureControl,
) -> Result<layer_color::PreparedDocumentColor, String> {
    let extent = [project.document.width, project.document.height];
    let source_space = project.document.color.space;
    let destination = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: color.depth,
        profile: ColorProfile::Builtin(color.space),
        profile_assumed: false,
    };
    let encoder = layer_color::WorkingEncoder::new(
        source_space,
        &destination,
        OutputEncoding {
            conversion: options,
            ..Default::default()
        },
    )?;
    let mut source = SourceBuilder::new(extent, destination, LIMIT)?;
    let mut renderer = gpu.capture(
        project,
        background,
        time,
        Default::default(),
        control.clone(),
    )
    .map_err(|e| e.to_string())?;
    let mut row = vec![0; extent[0] as usize * color.depth.bytes() * 4];
    let mut statistics = layer_color::OutputStatistics::default();
    for first in (0..extent[1]).step_by(16) {
        if control.is_cancelled() {
            return Err("Color conversion cancelled".into());
        }
        let height = 16.min(extent[1] - first);
        let pixels = renderer
            .read_region([0, first, extent[0], height])
            .map_err(|e| e.to_string())?;
        for (i, pixels) in pixels.chunks_exact(extent[0] as usize).enumerate() {
            statistics.clipped_channels += encoder
                .encode_premultiplied(pixels, &mut row, None, [0, first + i as u32])?
                .clipped_channels;
            source.push_row(&row)?;
        }
    }
    drop(renderer);
    if control.is_cancelled() {
        return Err("Color conversion cancelled".into());
    }
    let mut source = source.finish()?;
    source.kind = SourceKind::Rasterized;
    let allocated_bytes = source.resident_bytes();
    let mut document = layer_core::Document::new("Converted copy", extent[0], extent[1]);
    document.color = color;
    document.layers.truncate(1);
    document.layers[0].name = "Converted image".into();
    document.layers[0].source = Some(Arc::new(source));
    let project = Project {
        document,
        assets: Default::default(),
    };
    project.validate(Default::default())?;
    Ok(layer_color::PreparedDocumentColor {
        project,
        statistics,
        allocated_bytes,
    })
}
