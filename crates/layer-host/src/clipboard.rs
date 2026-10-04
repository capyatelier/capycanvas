//! Clipboard copies: frozen on the owner thread, composed and encoded on a
//! worker from an immutable snapshot, never on the UI thread.
use crate::Renderer;
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use layer_ui::{ClipboardCapture, PixelClip, UiSession};
use std::sync::Arc;

pub struct ClipTask {
    capture: ClipboardCapture,
    gpu: SnapshotGpu,
}

impl ClipTask {
    /// Freeze the pending Copy, Cut or Copy Merged request `request`.
    pub fn capture(session: &mut UiSession<Renderer>, request: u32) -> Result<Self, String> {
        let gpu = crate::tasks::gpu(session)?.snapshot_gpu();
        Self::new(session, request, gpu)
    }

    /// `capture` for a host whose canvas renderer lives elsewhere.
    pub fn new<R: layer_render::CanvasRenderer>(
        session: &mut UiSession<R>,
        request: u32,
        gpu: SnapshotGpu,
    ) -> Result<Self, String> {
        Ok(Self { capture: session.capture_clipboard(request)?, gpu })
    }

    pub fn capture_details(&self) -> &ClipboardCapture {
        &self.capture
    }

    /// Worker-side composition and encoding. `nonce` identifies the clip on
    /// the system clipboard.
    pub fn run(self, nonce: String, control: CaptureControl) -> Result<PixelClip, String> {
        let capture = self.capture;
        let limit = layer_color::photo::PhotoMemoryBudget::current().encode_bytes;
        let color = capture.color();
        if let Some(original) = capture.original.clone().filter(|_| !color.depth.is_float()) {
            let png = layer_color::source_png(&original, color, limit, || control.is_cancelled())?;
            return Ok(capture.finish(nonce, original, png));
        }
        let mut renderer = self
            .gpu
            .capture(capture.project.clone(), capture.time, control)
            .map_err(|e| e.to_string())?;
        let (source, png) =
            renderer.write_clip(capture.crop, capture.coverage.as_ref(), capture.original.is_none(), limit)?;
        drop(renderer);
        let source = match (&capture.original, source) {
            (Some(original), _) => original.clone(),
            (None, Some(source)) => Arc::new(source),
            (None, None) => return Err("The copy has no pixels".into()),
        };
        Ok(capture.finish(nonce, source, png))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeHost;
    use layer_core::{
        Point, Selection,
        color::{
            ColorProfile, SampleDepth,
            source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation, SourceKind},
        },
    };
    use layer_render_wgpu::WgpuRasterizer;
    use layer_ui::{CommandId, DocumentRequest, HostRequestKind, PasteMode, UiAction};

    fn host(document: layer_core::Document) -> NativeHost {
        let gpu = WgpuRasterizer::new_native_headless(document.color).unwrap();
        let mut host = NativeHost::new(layer_ui::Platform::Gtk).unwrap();
        let extent = [document.width, document.height];
        host.session = UiSession::new(Renderer(Some(gpu.into())), document, extent, layer_ui::Platform::Gtk).unwrap();
        host
    }

    fn photo(extent: [u32; 2], pixel: impl Fn(u32, u32) -> [u8; 4]) -> Arc<SourceImage> {
        let interpretation = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: ColorProfile::default(),
            profile_assumed: false,
        };
        let mut builder = SourceBuilder::new(extent, interpretation, 1 << 24).unwrap();
        for y in 0..extent[1] {
            builder.push_row(&(0..extent[0]).flat_map(|x| pixel(x, y)).collect::<Vec<_>>()).unwrap();
        }
        Arc::new(builder.finish().unwrap())
    }

    fn photo_host(selection: Option<Selection>) -> NativeHost {
        let mut document = layer_core::Document::new("Clip", 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        for paper in document.layers.iter_mut().filter(|l| l.id == layer_core::LayerId(2)) {
            paper.visible = false;
        }
        document.layers[0].source = Some(photo([64, 48], |x, y| [(x * 4) as u8, (y * 5) as u8, 200, 255]));
        document.selection = selection;
        let mut host = host(document);
        host.session.frame(0, 0).unwrap();
        host
    }

    fn copy(host: &mut NativeHost, command: CommandId) -> PixelClip {
        host.dispatch(UiAction::Invoke { command }).unwrap();
        let id = host
            .session
            .state()
            .requests
            .iter()
            .find(|r| matches!(r.kind, HostRequestKind::Document { request: DocumentRequest::Copy { .. } }))
            .unwrap()
            .id;
        let task = ClipTask::capture(&mut host.session, id).unwrap();
        let clip = task.run("nonce".into(), Default::default()).unwrap();
        host.session.complete_document_request(id, Ok(true)).unwrap();
        clip
    }

    fn rows(source: &SourceImage) -> Vec<Vec<u8>> {
        let mut rows = source.rows();
        (0..source.extent[1])
            .map(|y| {
                let mut row = vec![0; source.row_bytes()];
                rows.read(y, &mut row).unwrap();
                row
            })
            .collect()
    }

    #[test]
    fn copy_crops_multiplies_coverage_and_pastes_in_place() {
        let mut host = photo_host(None);
        host.dispatch(UiAction::Invoke { command: CommandId::SelectAll }).unwrap();
        let whole = copy(&mut host, CommandId::Copy);
        let photo = host.session.engine().document().layers.iter().find_map(|l| l.source.clone()).unwrap();
        assert!(Arc::ptr_eq(&whole.source, &photo), "an untouched photo keeps its original samples");
        assert_eq!(whole.origin, [0, 0]);
        let png = layer_color::photo::read_photo(std::io::Cursor::new(whole.png.to_vec()), Default::default()).unwrap();
        assert_eq!(png.extent, [64, 48]);
        let mut row = vec![0; png.row_bytes()];
        png.rows().read(8, &mut row).unwrap();
        assert!(row[40..44].iter().zip([40, 40, 200, 255]).all(|(a, b)| a.abs_diff(b) <= 1), "the photo's own pixels: {:?}", &row[40..44]);

        let selection = Selection::polygon(
            [[10., 8.], [30., 8.], [30., 20.], [10., 20.]].map(|[x, y]| Point { x, y }).to_vec(),
        )
        .unwrap();
        drop(host);
        let mut host = photo_host(Some(selection));
        let clip = copy(&mut host, CommandId::Copy);
        assert_eq!((clip.origin, clip.source.extent, clip.source.kind), ([10, 8], [20, 12], SourceKind::Rasterized));
        let copied = rows(&clip.source);
        for (y, row) in copied.iter().enumerate() {
            for (x, pixel) in row.chunks_exact(4).enumerate() {
                let expected = [((x + 10) * 4) as u8, ((y + 8) * 5) as u8, 200, 255];
                assert!(pixel.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1), "({x},{y}) {pixel:?} {expected:?}");
            }
        }

        let mut other = host_with_color(layer_core::color::DocumentColor {
            space: layer_core::color::RgbSpace::DisplayP3,
            depth: layer_core::color::SampleDepth::U16,
        });
        other.session.paste_clip(&clip, PasteMode::InPlace).unwrap();
        let document = other.session.engine().document();
        let pasted = document.layer(document.active_layer).unwrap();
        assert_eq!(pasted.source.as_ref().unwrap().kind, SourceKind::Original, "another colour mode converts");
        assert_eq!(pasted.properties.placement.as_affine().unwrap().0[4..], [10., 8.]);
        drop((host, other));
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }

    fn host_with_color(color: layer_core::color::DocumentColor) -> NativeHost {
        let mut document = layer_core::Document::new("Other", 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.color = color;
        host(document)
    }

    #[test]
    fn soft_coverage_and_copy_merged_include_what_is_visible() {
        let words = vec![0x80ff_ff80u32; 16 * 48];
        let pixels = layer_core::SelectionPixels::bytes([64, 48], [0, 0, 64, 48], words).unwrap();
        let mut host = photo_host(Some(Selection::pixels(Arc::new(pixels))));
        let clip = copy(&mut host, CommandId::CopyMerged);
        assert_eq!(clip.source.extent, [64, 48]);
        let copied = rows(&clip.source);
        assert!((127..=129).contains(&copied[3][3]), "half coverage: {:?}", &copied[3][..8]);
        assert_eq!(copied[3][7], 255, "full coverage");
        assert_eq!(clip.name, "Merged copy");
        drop(host);
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
