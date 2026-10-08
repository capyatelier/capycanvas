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

    pub fn set_evaluation_context(&mut self, mut context: layer_core::authored::EvaluationContext) {
        context.retain_effects(&self.capture.scene.artwork);
        Arc::make_mut(&mut self.capture.scene).context = context;
    }

    /// Worker-side composition and encoding. `nonce` identifies the clip on
    /// the system clipboard.
    pub fn run(self, nonce: String, control: CaptureControl) -> Result<PixelClip, String> {
        let mut capture = self.capture;
        for (id, layer) in capture.layer_captures() {
            let (source, _) = Self::pixels(&self.gpu, &layer, control.clone())?;
            capture.set_layer_source(id, source)?;
        }
        capture.finish_layer_sources()?;
        let (source, png) = Self::pixels(&self.gpu, &capture, control)?;
        capture.finish(nonce, source, png)
    }

    fn pixels(gpu: &SnapshotGpu, capture: &ClipboardCapture, control: CaptureControl)
        -> Result<(Arc<layer_core::color::source::SourceImage>, Vec<u8>), String> {
        let limit = layer_color::photo::PhotoMemoryBudget::current().encode_bytes;
        let color = capture.color();
        if let Some(original) = capture.original.clone().filter(|_| !color.depth.is_float()) {
            let png = layer_color::source_png(&original, color, limit, || control.is_cancelled())?;
            return Ok((original, png));
        }
        let mut renderer = gpu.capture_scene(capture.scene.clone(), capture.scope.clone(), control).map_err(|e| e.to_string())?;
        if let Some((origin, extent)) = capture.window { renderer.capture_window(origin, extent).map_err(|e| e.to_string())?; }
        let (source, png) = renderer.write_clip(capture.crop, capture.coverage.as_ref(), capture.original.is_none(), limit)?;
        let source = match (&capture.original, source) {
            (Some(original), _) => original.clone(),
            (None, Some(source)) => Arc::new(source),
            (None, None) => return Err("The copy has no pixels".into()),
        };
        Ok((source, png))
    }
}

#[cfg(test)]
mod tests {
    mod regions;
    use super::*;
    use crate::NativeHost;
    use layer_core::{
        Point, Selection,
        color::{
            ColorProfile, SampleDepth,
            source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation},
        },
    };
    use layer_render_wgpu::WgpuRasterizer;
    use layer_ui::{CommandId, DocumentRequest, HostRequestKind, PasteMode, UiAction};

    fn host(document: layer_core::Document) -> NativeHost {
        let gpu = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        let mut host = NativeHost::new(layer_ui::Platform::Gtk).unwrap();
        let extent = [document.composition().size[0], document.composition().size[1]];
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
        let mut document = layer_core::Document::new(layer_core::authored::PortableId::random(), 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        crate::test_support::hide_paper(&mut document);
        crate::test_support::active_source_mut(&mut document).base = Some(layer_core::authored::PaintBase::new(photo([64, 48], |x, y| [(x * 4) as u8, (y * 5) as u8, 200, 255]).into()));
        document.working.selection = selection;
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
        let photo = host.session.engine().document().artwork.paint.iter().find_map(|(_, _, source)| source.base.as_ref().map(|base|base.image.storage().clone())).unwrap();
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
        assert_eq!((clip.origin, clip.source.extent, clip.policy), ([10, 8], [20, 12], layer_core::authored::PaintBasePolicy::WorkingPixels));
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
        let pasted = crate::test_support::active_source(document);
        assert_eq!(pasted.base.as_ref().unwrap().policy, layer_core::authored::PaintBasePolicy::SourceProfile, "another colour mode converts");
        assert_eq!(document.target_offset(document.working.target.unwrap()), [10, 8]);
        drop((host, other));
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }

    fn host_with_color(color: layer_core::color::DocumentColor) -> NativeHost {
        let mut document = layer_core::Document::new(layer_core::authored::PortableId::random(), 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        crate::test_support::composition_mut(&mut document).color = color;
        host(document)
    }

    #[test]
    fn whole_group_clip_keeps_off_canvas_pixels_and_editable_layers_in_new_images() {
        use layer_core::{Edit, Occurrence, OccurrenceContent, RecordChange, Stack};
        let mut owner = photo_host(None);
        let doc = owner.session.engine().document();
        let paint = doc.working.occurrence.unwrap();
        let stack = RecordChange::insert(&doc.artwork.stacks, Stack { entries: vec![paint] });
        let group = RecordChange::insert(&doc.artwork.occurrences, Occurrence::new(OccurrenceContent::Stack(stack.handle), "Copied folder"));
        let group_id = group.handle;
        let mut root = doc.artwork.stacks.get(doc.composition().result).unwrap().clone();
        root.entries.retain(|h| *h != paint); root.entries.insert(0, group_id);
        let mut pixels = doc.scene().occurrence(paint).unwrap().clone(); pixels.offset = [80, -4];
        let mut working = doc.working.clone(); working.occurrence = Some(group_id); working.target = None;
        working.layer_selection = [group_id].into(); working.layer_anchor = Some(group_id);
        let edit = Edit::Batch(vec![Edit::Stack(stack), Edit::Occurrence(group),
            Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, paint, Some(pixels)).unwrap()),
            Edit::Stack(RecordChange::replace(&doc.artwork.stacks, doc.composition().result, Some(root)).unwrap()), Edit::Working(working)]);
        let mut document = doc.clone(); document.apply(edit).unwrap();
        drop(owner); owner = host(document); owner.session.frame(0, 0).unwrap();
        let clip = copy(&mut owner, CommandId::Copy);
        assert!(clip.layers.is_some());
        assert!(clip.origin[0] <= 80 && clip.origin[1] <= -4);
        assert!(clip.origin[0] + i64::from(clip.source.extent[0]) >= 144);
        let png = layer_color::photo::read_photo(std::io::Cursor::new(clip.png.to_vec()), Default::default()).unwrap();
        let [x, y] = [90 - clip.origin[0], 4 - clip.origin[1]].map(|v| v as usize);
        let png_rows = rows(&png);
        assert!(png_rows[y][x * 4..x * 4 + 4].iter().zip([40u8, 40, 200, 255]).all(|(a, b)| a.abs_diff(b) <= 1));
        let opened = clip.document(owner.session.localization()).unwrap();
        let folder = opened.working.occurrence.unwrap();
        assert_eq!(opened.scene().occurrence(folder).unwrap().name.as_ref(), "Copied folder");
        let children = opened.scene().children(Some(folder)); assert_eq!(children.len(), 1);
        assert_eq!(opened.layer_offset(children[0]), [80 - clip.origin[0], -4 - clip.origin[1]]);
        assert!(Arc::ptr_eq(opened.scene().paint_source(children[0]).unwrap().base.as_ref().unwrap().image.storage(),
            owner.session.engine().document().scene().paint_source(paint).unwrap().base.as_ref().unwrap().image.storage()));
        drop(owner); layer_render_wgpu::finish_shader_compiler_shutdown();
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

    #[test]
    fn copied_images_render_their_signed_window_without_changing_the_frame() {
        let mut document = layer_core::Document::new(layer_core::authored::PortableId::random(), 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        crate::test_support::hide_paper(&mut document);
        let (layer, edit) = document.create_object_layer_edit("Images", None, 0).unwrap();
        document.apply(edit).unwrap();
        let mut object = layer_core::ImageObject::new(photo([8, 6], |x, y| [(x * 30) as u8, (y * 40) as u8, 90, 255]).into(), "Photo");
        object.affine = layer_core::Affine64([1., 0., 0., 1., -5., -3.]);
        let (handle, edit) = document.add_image_object_edit(layer, object, 0).unwrap();
        document.apply(edit).unwrap();
        let mut working = document.working.clone();
        working.occurrence = Some(layer);
        working.target = None;
        working.objects = [handle].into();
        document.apply(layer_core::Edit::Working(working)).unwrap();
        let mut host = host(document);
        host.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: layer_ui::LayerCanvasTool::Move } }).unwrap();
        host.session.frame(0, 0).unwrap();
        let clip = copy(&mut host, CommandId::Copy);
        assert_eq!(clip.origin, [-5, -3]);
        assert_eq!(clip.source.extent, [8, 6]);
        assert_eq!(clip.objects.as_ref().unwrap().objects.len(), 1);
        let copied = rows(&clip.source);
        for (y, row) in copied.iter().enumerate() {
            for (x, pixel) in row.chunks_exact(4).enumerate() {
                let expected = [(x * 30) as u8, (y * 40) as u8, 90, 255];
                assert!(pixel.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1), "({x},{y}) {pixel:?} {expected:?}");
            }
        }
        assert_eq!(host.session.engine().document().composition().size, [64, 48]);
        drop(host);
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
