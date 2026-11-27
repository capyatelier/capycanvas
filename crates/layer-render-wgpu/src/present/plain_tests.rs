use super::*;
use crate::layer_tests::{page_bytes, placement::{paint_document, set_source}};
use crate::test_support::{packet, submit_settled};
use layer_core::color::{ColorProfile, DocumentColor, RgbSpace, SampleDepth};

const SURROUND: [f32; 4] = [0.2, 0.3, 0.4, 1.];

pub(crate) struct Drawn { pub shown: Vec<u8>, pub full: Vec<u8>, pub plain: Vec<u8>, pub used_plain: bool }

pub(crate) fn plain_ready(renderer: &WgpuRasterizer, format: wgpu::TextureFormat) -> ViewportPresenter {
    let mut presenter = ViewportPresenter::for_surface(renderer, format, SdrSurfaceColor::Srgb).unwrap();
    pollster::block_on(presenter.prepare(renderer)).unwrap();
    let lit = presenter.bound_viewport(renderer).1;
    pollster::block_on(renderer.compilation.prepare_all([presenter.plain_pipeline(renderer, lit)])).unwrap();
    presenter
}

fn drawn_with(presenter: &ViewportPresenter, renderer: &WgpuRasterizer, pipeline: &wgpu::RenderPipeline, extent: [u32; 2]) -> Vec<u8> {
    let (texture, target) = crate::create_target(renderer.device(), extent, presenter.format, "one viewport pipeline");
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("one viewport pipeline"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &target, depth_slice: None, resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
        depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, presenter.bind_group.as_ref().unwrap(), &[0]);
    pass.draw(0..3, 0..1);
    drop(pass);
    renderer.queue.submit([encoder.finish()]);
    page_bytes(renderer, &texture)
}

pub(crate) fn present(presenter: &mut ViewportPresenter, renderer: &WgpuRasterizer, view: ViewState) -> Drawn {
    let extent = [view.width_px, view.height_px];
    let (texture, target) = crate::create_target(renderer.device(), extent, presenter.format, "plain viewport");
    pollster::block_on(presenter.prepare(renderer)).unwrap();
    presenter.present(renderer, &target, view, SURROUND).unwrap();
    let (_, lit, full) = presenter.bound_viewport(renderer);
    let plain = presenter.plain_pipeline(renderer, lit);
    Drawn {
        shown: page_bytes(renderer, &texture),
        full: drawn_with(presenter, renderer, full.compile(), extent),
        plain: drawn_with(presenter, renderer, plain.compile(), extent),
        used_plain: presenter.last_artwork_pipeline.as_ref() == Some(plain.compile()),
    }
}

pub(crate) fn full_viewport_failure(drawn: &Drawn, feature: &str) -> Option<String> {
    let differing = |other: &[u8]| drawn.shown.chunks(4).zip(other.chunks(4)).filter(|(a, b)| a != b).count();
    let (from_full, from_plain) = (differing(&drawn.full), differing(&drawn.plain));
    (from_full != 0 || from_plain == 0 || drawn.used_plain).then(|| format!(
        "{feature}: {from_full} pixels differ from the full viewport, {from_plain} from the plain one, plain used: {}", drawn.used_plain))
}

fn stroke(extent: [u32; 2], x: u32, y: u32) -> [f32; 4] {
    let across = (y as f32 - 0.6 * x as f32 - 20.).abs() / 1.17;
    let along = x as f32 / extent[0] as f32;
    [2.4 * (1. - along) + 0.1, 0.2 + 0.7 * along, 0.35, (1. - across / 14.).clamp(0., 1.)]
}

fn local_tone(renderer: &WgpuRasterizer, extent: [u32; 2]) -> std::sync::Arc<crate::local_tone::GpuToneGuide> {
    let guide = layer_color::build_local_tone_guide(extent, RgbSpace::Srgb, || false, |y, row| {
        for (x, texel) in row.iter_mut().enumerate() {
            let [r, g, b, a] = stroke(extent, x as u32, y);
            *texel = [r * a, g * a, b * a, a];
        }
        Ok(())
    }).unwrap();
    std::sync::Arc::new(crate::local_tone::GpuToneGuide::from_cpu(renderer, &guide))
}

#[test]
fn plain_frames_present_the_full_viewport_pixels_bit_for_bit() {
    let extent = [128; 2];
    for depth in [SampleDepth::U8, SampleDepth::F32] {
        for paper in [false, true] {
            let mut document = if paper {
                let mut sheet = crate::layer_tests::paper_shade::rough_gray_paper();
                crate::layer_tests::paper_shade::set_paper(&mut sheet, &[(layer_core::paper::RELIEF, layer_core::EffectValue::Number(150.)),
                    (layer_core::paper::THROUGH_PAINT, layer_core::EffectValue::Number(0.5))]);
                sheet
            } else { paint_document(extent, "Plain stroke") };
            let root = document.artwork.root;
            document.artwork.compositions.get_mut(root).unwrap().color.depth = depth;
            set_source(&mut document, crate::test_support::depth_source(extent, depth, RgbSpace::Srgb, 128 * 128 * 16 + 4096, |x, y| stroke(extent, x, y)));
            let mut renderer = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
            submit_settled(&mut renderer, packet(document.scene(), extent));
            for format in [wgpu::TextureFormat::Rgba8UnormSrgb, wgpu::TextureFormat::Rgba16Float] {
                let case = format!("{depth:?} document, paper {paper}, {format:?} target");
                let mut presenter = plain_ready(&renderer, format);
                presenter.set_gpu_local_tone_guide(&renderer, (depth == SampleDepth::F32).then(|| local_tone(&renderer, extent))).unwrap();
                assert_eq!(presenter.bound_viewport(&renderer).1, paper, "{case}: paper lighting selects the lit variants");
                let drawn = present(&mut presenter, &renderer, crate::test_support::view(extent));
                let colors: std::collections::HashSet<&[u8]> = drawn.shown.chunks(4).collect();
                assert!(colors.len() > 64, "{case}: the stroke, its soft edges and the canvas behind it show: {} colors", colors.len());
                assert!(drawn.used_plain, "{case}: a frame without overlays or previews presents through the plain viewport");
                assert!(drawn.shown == drawn.full && drawn.plain == drawn.full, "{case}: the plain viewport draws the full viewport's pixels");
            }
        }
    }
}

#[test]
fn every_overlay_and_preview_presents_through_the_full_viewport() {
    use layer_core::{Affine, Edit, RecordChange, SavedSelection, Selection, SelectionPixels, authored::{Occurrence, OccurrenceContent, SelectionOverlays}};
    fn frame<'a>(document: &'a layer_core::Document, overlays: &'a SelectionOverlays) -> layer_render::FramePacket<'a> {
        layer_render::FramePacket { selection_overlays: Some(overlays), ..packet(document.scene(), [128; 2]) }
    }
    let extent = [128; 2];
    let color = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 };
    let image = |pixel: fn(u32, u32) -> [f32; 4]| crate::test_support::depth_source(extent, color.depth, color.space, 128 * 128 * 8 + 4096, pixel);
    let mut document = paint_document(extent, "Overlays");
    let root = document.artwork.root;
    document.artwork.compositions.get_mut(root).unwrap().color = color;
    set_source(&mut document, image(|x, _| match x {
        0..32 => [0., 1., 0., 1.], 32..48 => [0., 0., 0., 1.], 48..64 => [1.; 4], 64..96 => [0.4, 0.5, 0.6, 1.], _ => [0.; 4],
    }));
    let saved = RecordChange::insert(&document.artwork.selections, SavedSelection { selection: Selection::full() });
    let occurrence = RecordChange::insert(&document.artwork.occurrences, Occurrence::new(OccurrenceContent::Selection(saved.handle), "Saved"));
    let handle = occurrence.handle;
    let stack = document.composition().result;
    let mut members = document.artwork.stacks.get(stack).unwrap().clone();
    members.entries.push(handle);
    let members = RecordChange::replace(&document.artwork.stacks, stack, Some(members)).unwrap();
    document.apply(Edit::Batch(vec![Edit::SavedSelection(saved), Edit::Occurrence(occurrence), Edit::Stack(members)])).unwrap();
    let hidden = SelectionOverlays { visibility: [(handle, false)].into(), ..Default::default() };
    let shown = SelectionOverlays { visibility: [(handle, true)].into(), ..Default::default() };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    submit_settled(&mut r, frame(&document, &hidden));
    let view = crate::test_support::view(extent);
    let mut presenter = plain_ready(&r, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut wrong = Vec::new();
    let mut check = |presenter: &mut ViewportPresenter, r: &WgpuRasterizer, feature: &str| {
        wrong.extend(full_viewport_failure(&present(presenter, r, view), feature));
    };
    let cleared = |presenter: &mut ViewportPresenter, r: &WgpuRasterizer, feature: &str| {
        assert!(present(presenter, r, view).used_plain, "after {feature}, the plain viewport presents again");
    };
    cleared(&mut presenter, &r, "the first frame");
    let inside = |x: u32, y: u32| (32..96).contains(&x) && (32..96).contains(&y);
    let bytes: Vec<u8> = (0..128 * 128).map(|i| if inside(i % 128, i / 128) { 255 } else { 0 }).collect();
    let words: Vec<u32> = bytes.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
    let selection = Selection::pixels(std::sync::Arc::new(SelectionPixels::bytes(extent, [32, 32, 96, 96], words).unwrap()));
    let overlay = layer_render::SelectionOverlay { active: true, editing: None, color: [1., 0., 0., 0.5], protected: false, saved_protected: false };
    r.set_selection_outline(Some(&selection)).unwrap();
    check(&mut presenter, &r, "a selection outline");
    r.set_selection_overlay(Some(overlay));
    check(&mut presenter, &r, "a selection tint");
    r.set_selection_outline(None).unwrap();
    r.set_selection_overlay(None);
    cleared(&mut presenter, &r, "a selection");
    r.set_selection_overlay(Some(layer_render::SelectionOverlay { active: false, ..overlay }));
    submit_settled(&mut r, frame(&document, &shown));
    assert!(r.selection_previews.texture.is_some());
    check(&mut presenter, &r, "a saved selection");
    submit_settled(&mut r, frame(&document, &hidden));
    r.set_selection_overlay(None);
    cleared(&mut presenter, &r, "a saved selection");
    r.set_crop_overlay(Some(layer_render::CropOverlay { to_crop: Affine([96., 0., 0., 96., 16., 16.]).inverse().unwrap(), dim: 0.5 }));
    check(&mut presenter, &r, "a crop shield");
    r.set_crop_overlay(None);
    cleared(&mut presenter, &r, "a crop shield");
    let lut = std::sync::Arc::new(layer_color::ProofLut::build(color.space,
        &layer_core::color::ProofRecipe::new("sRGB gamut".into(), ColorProfile::Builtin(RgbSpace::Srgb)), || false).unwrap());
    presenter.set_proof(&r, Some(lut.clone()), true, false).unwrap();
    check(&mut presenter, &r, "a soft proof");
    presenter.set_proof(&r, Some(lut), false, true).unwrap();
    check(&mut presenter, &r, "a gamut warning");
    presenter.set_proof(&r, None, false, false).unwrap();
    cleared(&mut presenter, &r, "a proof");
    let screen = layer_color::screen::Chromaticities::of(RgbSpace::Srgb).from_space(RgbSpace::DisplayP3);
    presenter.set_screen_check(&r, Some(crate::ScreenCheck { from_view: Some(screen), surface_clips: true, bounded: true, mark: true }));
    check(&mut presenter, &r, "screen marks");
    presenter.set_screen_check(&r, None);
    cleared(&mut presenter, &r, "screen marks");
    for (shadows, highlights) in [(true, false), (false, true)] {
        r.set_clipping_preview(shadows, highlights);
        check(&mut presenter, &r, &format!("clipping stripes for shadows {shadows}, highlights {highlights}"));
    }
    r.set_clipping_preview(false, false);
    cleared(&mut presenter, &r, "clipping stripes");
    let reveal = layer_render::FillReveal { transaction: 7, center: [64.5, 64.5], radius: -1. };
    r.set_fill_reveal(Some(reveal));
    pollster::block_on(presenter.prepare(&r)).unwrap();
    presenter.present(&r, &crate::create_target(r.device(), extent, presenter.format, "fill before").1, view, SURROUND).unwrap();
    assert_eq!(presenter.fill_before.as_ref().map(|(id, _)| *id), Some(7));
    set_source(&mut document, image(|_, y| if y < 64 { [0.9, 0.2, 0.1, 1.] } else { [0.1, 0.2, 0.9, 0.5] }));
    submit_settled(&mut r, frame(&document, &hidden));
    r.set_fill_reveal(Some(layer_render::FillReveal { radius: 24., ..reveal }));
    check(&mut presenter, &r, "a fill reveal");
    r.set_fill_reveal(None);
    cleared(&mut presenter, &r, "a fill reveal");
    assert!(wrong.is_empty(), "{wrong:#?}");
}
