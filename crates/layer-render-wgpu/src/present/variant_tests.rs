use super::*;
use layer_core::{Affine, ImageTransform, Point};
use layer_core::authored::{OccurrenceContent, SourceTarget};
use std::time::{Duration, Instant};

struct VariantFixture {
    document: layer_core::Document,
    renderer: WgpuRasterizer,
    view: ViewState,
    texture: wgpu::Texture,
    target: wgpu::TextureView,
}

impl VariantFixture {
    fn new(mode: usize) -> Self {
        let extent = [128; 2];
        let document = crate::scene::scale::tests::document_at(extent);
        let mut renderer = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        let initial = crate::test_support::packet(document.scene(), extent);
        crate::test_support::submit_settled(&mut renderer, initial);
        let view = initial.view;
        let (texture, target) = crate::create_target(&renderer.device, extent, wgpu::TextureFormat::Rgba8UnormSrgb, "viewport projection");
        let mut fixture = Self { document, renderer, view, texture, target };
        if mode != 0 { fixture.expose(mode); }
        fixture.assert_projection();
        fixture.renderer.shader_idle(false, false);
        fixture
    }

    fn presenter(&self) -> ViewportPresenter {
        let presenter = ViewportPresenter::for_surface(&self.renderer, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb).unwrap();
        assert!(presenter.pipeline.iter().all(|pipeline| !pipeline.ready()), "a fresh renderer must leave every viewport recipe cold");
        presenter
    }

    fn expose(&mut self, mode: usize) {
        self.view.document_to_surface = [0.25, 0., 0., 0.25, 16., 16.];
        let owner = self.document.scene().order()[0];
        let OccurrenceContent::Paint(source) = self.document.scene().occurrence(owner).unwrap().content else {
            panic!("the viewport fixture requires one paint source");
        };
        if mode == 1 {
            self.document.artwork.occurrences.get_mut(owner).unwrap().offset = [1, 0];
            self.renderer.prepare_moving_layer(Some(owner));
        } else {
            assert_eq!(mode, 2);
            self.renderer.set_transform_preview(Some(&layer_render::TransformPreview {
                transaction: 1, target: SourceTarget::Paint(source), moving: true, selection: None,
                transform: ImageTransform::affine(Affine::translation(Point { x: 10., y: 0. })),
            })).unwrap();
        }
        crate::test_support::submit_settled(&mut self.renderer, layer_render::FramePacket {
            view: self.view, ..crate::test_support::packet(self.document.scene(), self.document.composition().size)
        });
        self.assert_projection();
    }

    fn assert_projection(&self) {
        let page = self.renderer.presentation_page().expect("the producer must publish committed keyed pixels");
        assert!(page.geometry().size() > 0);
        assert!(page.views().iter().all(|view| view.texture().width() > 0 && view.texture().height() > 0));
        assert!(self.renderer.selected_display_cache().unwrap().presentable_pages() > 0);
    }

    fn encode(&self, presenter: &mut ViewportPresenter) -> wgpu::SubmissionIndex {
        let mut encoder = self.renderer.device.create_command_encoder(&Default::default());
        presenter.encode(&self.renderer, &mut encoder, &self.target, self.view, [0.; 4]).unwrap();
        self.renderer.queue.submit([encoder.finish()])
    }

    fn complete(&self, submission: wgpu::SubmissionIndex) {
        self.renderer.device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission), timeout: Some(Duration::from_secs(30)),
        }).unwrap();
    }

    fn assert_binding(&self, presenter: &ViewportPresenter) {
        self.assert_projection();
        assert_eq!(presenter.camera_data.unwrap()[53], 0.);
        assert_eq!(presenter.last_artwork_pipeline.as_ref(), Some(presenter.pipeline[0].compile()));
    }

    fn pixels(&self) -> Vec<u8> { crate::layer_tests::page_bytes(&self.renderer, &self.texture) }

    fn assert_pixels(&self) {
        let mut reference = ViewportPresenter::for_surface(&self.renderer, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb).unwrap();
        let expected = reference.readback_rgba8(&self.renderer, self.view, [0.; 4]).unwrap();
        assert_eq!(self.pixels(), expected.bytes);
    }
}

fn prepare_projection(renderer: &WgpuRasterizer, presenter: &ViewportPresenter) {
    pollster::block_on(renderer.compilation.prepare_all([
        &presenter.pipeline[0], &presenter.surround_pipeline,
        &presenter.cursor_pipeline, &presenter.recipes.transition.pipeline,
    ])).unwrap();
}

fn wait_for_projection(renderer: &WgpuRasterizer, presenter: &ViewportPresenter) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !presenter.pipelines_ready(renderer) {
        renderer.compilation.check().unwrap();
        assert!(Instant::now() < deadline, "the keyed viewport recipe must become ready");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn actual_viewport_sources_match_the_predicted_and_bound_variant() {
    for mode in 0..3 {
        let fixture = VariantFixture::new(mode);
        let batch = fixture.renderer.compilation.batch();
        let mut presenter = fixture.presenter();
        assert!(!presenter.pipelines_ready(&fixture.renderer));
        prepare_projection(&fixture.renderer, &presenter);
        assert!(presenter.pipelines_ready(&fixture.renderer));
        assert!(presenter.pipeline[1..].iter().all(|pipeline| !pipeline.ready()), "unused viewport variants must not hold this frame");
        let submission = fixture.encode(&mut presenter);
        fixture.assert_binding(&presenter);
        fixture.complete(submission);
        fixture.assert_pixels();
        drop(batch);
    }
    crate::finish_shader_compiler_shutdown();
}

#[test]
fn cold_keyed_projection_preserves_pixels_and_history_until_ready() {
    for mode in 0..3 {
        let fixture = VariantFixture::new(mode);
        let batch = fixture.renderer.compilation.batch();
        let mut presenter = fixture.presenter();
        pollster::block_on(fixture.renderer.compilation.prepare_all([
            &presenter.surround_pipeline, &presenter.cursor_pipeline, &presenter.recipes.transition.pipeline,
        ])).unwrap();
        assert!(!presenter.pipelines_ready(&fixture.renderer));
        let previous = fixture.pixels();
        let camera = presenter.camera_data;
        let bound = presenter.last_artwork_pipeline.clone();
        let submission = fixture.encode(&mut presenter);
        assert_eq!(presenter.camera_data, camera);
        assert_eq!(presenter.last_artwork_pipeline, bound);
        assert!(!presenter.history.valid);
        assert!(!presenter.pipeline[0].ready(), "encoding cannot compile the cold viewport inline");
        fixture.complete(submission);
        assert_eq!(fixture.pixels(), previous);
        prepare_projection(&fixture.renderer, &presenter);
        let submission = fixture.encode(&mut presenter);
        fixture.assert_binding(&presenter);
        assert!(presenter.history.valid);
        fixture.complete(submission);
        assert_ne!(fixture.pixels(), previous);
        fixture.assert_pixels();
        drop(batch);
    }
    crate::finish_shader_compiler_shutdown();
}

#[test]
fn immediate_viewport_source_switch_uses_the_ready_keyed_projection() {
    for mode in [1, 2] {
        let mut fixture = VariantFixture::new(0);
        let mut presenter = fixture.presenter();
        wait_for_projection(&fixture.renderer, &presenter);
        let submission = fixture.encode(&mut presenter);
        fixture.complete(submission);
        fixture.expose(mode);
        assert!(presenter.pipelines_ready(&fixture.renderer));
        let submission = fixture.encode(&mut presenter);
        fixture.complete(submission);
        fixture.assert_binding(&presenter);
        fixture.assert_pixels();
        assert!(presenter.pipeline[1..].iter().all(|pipeline|
            !crate::compile::tests::requested(&fixture.renderer.compilation, pipeline, crate::startup::DOCUMENT)));
    }
    crate::finish_shader_compiler_shutdown();
}
