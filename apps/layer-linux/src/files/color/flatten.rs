//! Share the portable full-composition copy writer with the other native hosts.
use super::*;
use layer_render_wgpu::snapshot::SnapshotGpu;

pub(super) fn prepare(
    gpu: SnapshotGpu,
    project: Project,
    color: DocumentColor,
    options: ConversionOptions,
    time: f32,
    control: CaptureControl,
) -> Result<layer_color::PreparedDocumentColor, String> {
    gpu.capture(project, time, control)
        .map_err(|e| e.to_string())?
        .flattened_document(
            color,
            options,
            layer_color::photo::PhotoMemoryBudget::current().encode_bytes,
        )
}
