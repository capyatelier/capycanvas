//! Share the portable full-composition copy writer with the other native hosts.
use super::*;
use layer_render_wgpu::snapshot::SnapshotGpu;

pub(super) fn prepare(
    gpu: SnapshotGpu,
    project: Document,
    color: DocumentColor,
    options: ConversionOptions,
    context: layer_core::authored::EvaluationContext,
    control: CaptureControl,
) -> Result<layer_color::PreparedDocumentColor, String> {
    gpu.capture(layer_host::tasks::capture_document_at(&project, context), control)
        .map_err(|e| e.to_string())?
        .flattened_document(
            color,
            options,
            layer_color::photo::PhotoMemoryBudget::current().encode_bytes,
        )
}
