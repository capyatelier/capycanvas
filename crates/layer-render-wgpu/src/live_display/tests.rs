use super::*;
use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace, source::*};
use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource};

#[test]
fn camera_rotation_preserves_power_of_two_detail_levels() {
    let plan = display_mips::Plan::new([8192, 7324]).unwrap();
    for level in 0..plan.level {
        let scale = 1. / (1u32 << level) as f32;
        for step in 0..1440 {
            let angle = step as f32 * std::f32::consts::TAU / 1440.;
            let (sin, cos) = angle.sin_cos();
            for reflect in [1., -1.] {
                let window = Window::new(view([
                    reflect * scale * cos, reflect * scale * sin,
                    -scale * sin, scale * cos, 160., 120.,
                ]), plan).unwrap().unwrap();
                assert_eq!(window.level, level, "level={level} angle={angle} reflect={reflect}");
            }
        }
        if level > 0 {
            // A real zoom crossing is not held at the coarser level. Include
            // nonuniform transforms: the largest stretch controls resolution.
            let larger = scale * 1.00001;
            let window = Window::new(view([larger, 0., 0., scale, 0., 0.]), plan)
                .unwrap().unwrap();
            assert_eq!(window.level, level - 1);
        }
    }
}

#[test]
fn retained_mips_match_windowed_pixels_and_zoomed_out_edits_invalidate_native_detail() {
    let mut doc = document([1537, 769]);
    let mut retained = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut windowed = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    for r in [&mut retained, &mut windowed] {
        r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    }
    windowed.native_edit.as_mut().unwrap().display_cache_bytes = DETAIL_BYTES;
    let mut a = ViewportPresenter::for_surface(&retained, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let mut b = ViewportPresenter::for_surface(&windowed, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let native = view([1., 0., 0., 1., -700., -300.]);
    for r in [&mut retained, &mut windowed] { submit(r, &doc, native, true); }
    assert!(!retained.live_display.as_ref().unwrap().retained.is_empty());
    assert!(windowed.live_display.as_ref().unwrap().retained.is_empty());
    let work = retained.metrics.composited_pixels;
    for matrix in [
        [0.5, 0., 0., 0.5, 0., 0.],
        [0.5, 0., 0., 0.5, -550., -150.],
        [0., 0.5, -0.5, 0., 350., -200.],
        [0.25, 0., 0., 0.25, 0., 0.],
    ] {
        let v = view(matrix);
        for r in [&mut retained, &mut windowed] { submit(r, &doc, v, false); }
        close(&present(&retained, &mut a, v), &present(&windowed, &mut b, v));
        assert_eq!(retained.metrics.composited_pixels, work,
            "retained display levels must not re-evaluate the document for navigation");
    }
    // The old native window is not visible during this edit. Returning to it
    // must reject stale detail, including when the new view uses retained mips.
    doc.layers[0].opacity = 0.35;
    let zoomed_out = view([0.25, 0., 0., 0.25, 0., 0.]);
    for r in [&mut retained, &mut windowed] {
        submit(r, &doc, zoomed_out, true);
        submit(r, &doc, native, false);
    }
    let mut dense = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut c = ViewportPresenter::for_surface(&dense, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    submit(&mut dense, &doc, native, true);
    let expected = present(&dense, &mut c, native);
    close(&expected, &present(&retained, &mut a, native));
    close(&expected, &present(&windowed, &mut b, native));
    // Force native cache misses without an artwork edit, including the partial
    // bottom/right source page. Native detail must match full composition while
    // already-completed coarse and retained levels remain bit-identical.
    let levels = |r: &WgpuRasterizer| {
        let cache = r.live_display.as_ref().unwrap();
        std::iter::once(&cache.coarse.texture)
            .chain(cache.retained.iter().map(|level| &level.texture))
            .map(|texture| pixels(r, texture))
            .collect::<Vec<_>>()
    };
    let before = levels(&retained);
    for v in [native, view([1., 0., 0., 1., -1350., -650.])] {
        retained.live_display.as_mut().unwrap().fine.as_mut().unwrap().keys.fill(None);
        let work = retained.metrics.composited_pixels;
        submit(&mut retained, &doc, v, false);
        submit(&mut dense, &doc, v, false);
        assert!(retained.metrics.composited_pixels > work);
        close(&present(&dense, &mut c, v), &present(&retained, &mut a, v));
        assert_eq!(before, levels(&retained));
    }
}

fn document(extent: [u32; 2]) -> layer_core::Document {
    let mut doc = layer_core::Document::new("bounded live display", extent[0], extent[1]);
    doc.color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: IntegerDepth::U16,
    };
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
            profile_assumed: false,
        },
        64 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..extent[1] {
        let mut row = Vec::with_capacity(extent[0] as usize * 8);
        for x in 0..extent[0] {
            for v in [
                ((x * 53 + y * 17) % 60000) as u16,
                ((x * 7 + y * 31) % 50000) as u16,
                (5000 + (x * 3 + y * 19) % 55000) as u16,
                40000,
            ] {
                row.extend_from_slice(&v.to_le_bytes());
            }
        }
        builder.push_row(&row).unwrap();
    }
    doc.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    doc
}
fn view(matrix: [f32; 6]) -> ViewState {
    ViewState {
        width_px: 320,
        height_px: 240,
        document_to_surface: matrix,
        background_rgba_linear: [1.; 4],
    }
}
fn submit(r: &mut WgpuRasterizer, doc: &layer_core::Document, view: ViewState, all: bool) {
    r.submit(FramePacket {
        layers: &doc.layers,
        document_extent: [doc.width, doc.height],
        view,
        time_seconds: 0.,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: false,
        composite_all: all,
    })
    .unwrap();
}
fn pixels(r: &WgpuRasterizer, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
    crate::layer_tests::page_bytes(r, texture)
        .chunks_exact(16)
        .map(|p| {
            std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect()
}
fn present(
    r: &WgpuRasterizer,
    presenter: &mut ViewportPresenter,
    view: ViewState,
) -> Vec<[f32; 4]> {
    let (texture, target) = create_target(
        &r.device,
        [view.width_px, view.height_px],
        wgpu::TextureFormat::Rgba32Float,
        "bounded display oracle target",
    );
    presenter.present(r, &target, view, [0.; 4]).unwrap();
    pixels(r, &texture)
}
fn close(a: &[[f32; 4]], b: &[[f32; 4]]) {
    assert_eq!(a.len(), b.len());
    for (pixel, (a, b)) in a.iter().zip(b).enumerate() {
        for i in 0..4 {
            assert!(
                (a[i] - b[i]).abs() < 5e-6,
                "pixel {pixel}, channel {i}: {} != {}",
                a[i],
                b[i]
            );
        }
    }
}

#[test]
fn visible_detail_matches_dense_composition_through_pan_wrap_rotation_and_resize() {
    let doc = document([1537, 769]);
    let mut dense = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut cached = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    cached.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    let mut a = ViewportPresenter::for_surface(
        &dense,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let mut b = ViewportPresenter::for_surface(
        &cached,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    for (index, matrix) in [
        [1., 0., 0., 1., 0., 0.],
        [1., 0., 0., 1., -100., -100.],
        [1., 0., 0., 1., -700., -450.],
        [1., 0., 0., 1., -1200., -529.],
        [1., 0., 0., 1., -256., -256.],
        [0., 1., -1., 0., 600., -700.],
        [-1., 0., 0., 1., 1200., -300.],
        [2., 0., 0., 2., -1111., -555.],
        [0.5, 0., 0., 0.5, -233., -100.],
        [1., 0., 0., 1., 0., 0.],
    ]
    .into_iter()
    .enumerate()
    {
        let view = view(matrix);
        submit(&mut dense, &doc, view, index == 0);
        submit(&mut cached, &doc, view, index == 0);
        assert!(cached.composite_texture.is_none());
        assert!(cached.live_display.as_ref().unwrap().storage_bytes() <= CACHE_BYTES);
        close(
            &present(&dense, &mut a, view),
            &present(&cached, &mut b, view),
        );
        if index > 0 {
            assert_eq!(
                dense.composite_revision, cached.composite_revision,
                "camera motion is not an artwork change"
            );
        }
    }
    let revision = cached.composite_revision;
    let work = cached.metrics.composited_pixels;
    submit(&mut cached, &doc, view([1., 0., 0., 1., -1., 0.]), false);
    assert_eq!(
        cached.metrics.composited_pixels, work,
        "resident pan must reuse detail"
    );
    assert_eq!(cached.composite_revision, revision);
    assert!(
        cached
            .request_color_sample(ColorSampleRequest {
                request_id: 11,
                source: ColorSampleSource::Composite,
                position: [255, 256],
                area: ColorSampleArea::Average5,
            })
            .unwrap()
    );
    cached
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    let sample = cached.take_color_sample().unwrap().unwrap();
    assert!(sample.rgba.iter().all(|v| v.is_finite()));
    assert!(cached.request_canvas_preview(None).unwrap());
    cached
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    assert!(
        cached
            .take_canvas_preview()
            .unwrap()
            .unwrap()
            .image
            .is_some()
    );
}

#[test]
fn large_live_composite_uses_bounded_pixels_and_zoom_out_matches_full_area_reference() {
    let doc = document([4097, 1025]);
    let mut cached = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    cached.native_edit.as_mut().unwrap().display_cache_bytes = DETAIL_BYTES;
    let v = view([0.25, 0., 0., 0.25, -256., 0.]);
    submit(&mut cached, &doc, v, true);
    assert!(
        cached.composite_texture.is_none(),
        "the default threshold must choose bounded storage"
    );
    // The constrained cache keeps a complete coarse image and visible detail;
    // it does not allocate the optional retained levels or seed native detail.
    assert!(cached.metrics.composite_storage_bytes < 16 * 1024 * 1024);
    let mut dense = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    dense.native_edit.as_mut().unwrap().display_dense_bytes = u64::MAX;
    submit(&mut dense, &doc, v, true);
    let working = pixels(&dense, dense.composite_texture.as_ref().unwrap());
    let mut expected = Vec::new();
    for y in 0..v.height_px {
        for x in 0..v.width_px {
            let mut rgb = [0f64; 3];
            for yy in y * 4..y * 4 + 4 {
                for xx in 1024 + x * 4..1024 + x * 4 + 4 {
                    for i in 0..3 {
                        rgb[i] += working[(yy * doc.width + xx) as usize][i] as f64 / 16.;
                    }
                }
            }
            let rgb = layer_core::color::rgb::apply(
                RgbSpace::DisplayP3.linear_transform(RgbSpace::Srgb),
                rgb,
            );
            expected.push([rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.]);
        }
    }
    let mut presenter = ViewportPresenter::for_surface(
        &cached,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    close(&expected, &present(&cached, &mut presenter, v));
}

#[test]
fn rejected_views_and_abandoned_cache_writes_preserve_artwork_and_rebuild_missing_detail() {
    let doc = document([1537, 769]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    r.native_edit.as_mut().unwrap().display_cache_bytes = 8 * 1024 * 1024;
    let v = view([1., 0., 0., 1., 0., 0.]);
    submit(&mut r, &doc, v, true);
    let mut presenter = ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let before = present(&r, &mut presenter, v);
    let bytes = r.live_display.as_ref().unwrap().storage_bytes();
    let rejected = r
        .submit(FramePacket {
            layers: &doc.layers,
            document_extent: [doc.width, doc.height],
            view: ViewState {
                width_px: 2048,
                height_px: 1536,
                ..v
            },
            time_seconds: 0.,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: false,
            composite_all: false,
        })
        .unwrap_err();
    assert!(rejected.to_string().contains("cache limit"));
    assert_eq!(r.live_display.as_ref().unwrap().storage_bytes(), bytes);
    close(&before, &present(&r, &mut presenter, v));

    let mut cache = r.live_display.take().unwrap();
    let source = create_color_target(&r.device, [PAGE_SIZE; 2], "abandoned display overwrite");
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.encode_clear_value(
        &mut encoder,
        &source.1,
        "unsubmitted white display tile",
        1.,
    );
    cache
        .write_tile(
            &r,
            r.display_pipelines.as_ref().unwrap(),
            &mut encoder,
            &source.0,
            [0; 2],
            [0; 2],
        )
        .unwrap();
    drop(encoder);
    r.live_display = Some(cache);
    let work = r.metrics.composited_pixels;
    submit(&mut r, &doc, v, false);
    assert_eq!(
        r.metrics.composited_pixels - work,
        u64::from(PAGE_SIZE).pow(2)
    );
    close(&before, &present(&r, &mut presenter, v));
    let mut dense = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    submit(&mut dense, &doc, v, true);
    assert_eq!(
        r.readback_srgb_rgba8().unwrap(),
        dense.readback_srgb_rgba8().unwrap(),
        "explicit readback captures artwork independently of either display cache"
    );
    close(&before, &present(&r, &mut presenter, v));
}

#[test]
fn cache_changes_shape_within_budget_and_rebinds_retired_presenter_views() {
    let doc = document([1537, 769]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    r.native_edit.as_mut().unwrap().display_cache_bytes = 8 * 1024 * 1024;
    let mut dense = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut a = ViewportPresenter::for_surface(
        &dense,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let mut b = ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    for (index, [width_px, height_px]) in [[1024, 240], [240, 768], [1024, 240]]
        .into_iter()
        .enumerate()
    {
        let v = ViewState {
            width_px,
            height_px,
            ..view([1., 0., 0., 1., 0., 0.])
        };
        submit(&mut dense, &doc, v, index == 0);
        submit(&mut r, &doc, v, index == 0);
        assert!(r.live_display.as_ref().unwrap().storage_bytes() <= 8 * 1024 * 1024);
        close(&present(&dense, &mut a, v), &present(&r, &mut b, v));
    }
}

#[test]
fn filtered_masked_source_edits_and_restoration_refresh_detail_and_coarse_display() {
    use layer_core::{LayerMask, Point, Selection};
    let mut doc = document([777, 533]);
    let source = doc.layers[0].source.clone().unwrap();
    let mut mask = LayerMask::reveal_all(LayerId(99), Point { x: 7., y: -9. });
    mask.default_coverage = 0.;
    mask.initial = Some(
        Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 760., y: 99. },
            Point { x: 440., y: 533. },
        ])
        .unwrap(),
    );
    doc.layers[0].mask = Some(mask);
    doc.layers
        .insert(0, crate::tests::image_windows::effect(20, false, false));
    doc.layers
        .insert(0, crate::tests::image_windows::effect(21, false, false));
    let original = doc.clone();
    let mut dense = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    let mut a = ViewportPresenter::for_surface(
        &dense,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let mut b = ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let v = view([1., 0., 0., 1., -233., -111.]);
    let mut first = Vec::new();
    for step in 0..5 {
        match step {
            1 => doc.layers[2].properties.offset = Point { x: 17., y: -9. },
            2 => {
                doc.layers[2].mask.as_mut().unwrap().inverted = true;
                doc.layers[0].opacity = 0.6;
            }
            3 => doc.layers[2].mask.as_mut().unwrap().show_area = true,
            4 => doc = original.clone(),
            _ => {}
        }
        // Layer property actions request recomposition through FramePacket;
        // only camera motion uses an otherwise clean packet.
        submit(&mut dense, &doc, v, true);
        submit(&mut r, &doc, v, true);
        let displayed = present(&r, &mut b, v);
        close(&present(&dense, &mut a, v), &displayed);
        if step == 0 {
            first = displayed;
        } else if step == 4 {
            close(&first, &displayed);
        } else {
            assert!(
                first != displayed,
                "step {step} must change visible artwork"
            );
        }
        // Both preview routes must reflect the same revised artwork, including
        // masked physical filter halos and the whole-image inspection display.
        for renderer in [&mut dense, &mut r] {
            assert!(renderer.request_canvas_preview(None).unwrap());
            renderer
                .device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
        }
        let expected = dense.take_canvas_preview().unwrap().unwrap().image.unwrap();
        let actual = r.take_canvas_preview().unwrap().unwrap().image.unwrap();
        assert!(
            expected.bytes == actual.bytes,
            "coarse preview at step {step}"
        );
        assert!(Arc::ptr_eq(doc.layers[2].source.as_ref().unwrap(), &source));
    }
    // Recreating the GPU cache reconstructs the same pixels from retained source.
    let mut recovered = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    recovered.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    submit(&mut recovered, &doc, v, true);
    let mut p = ViewportPresenter::for_surface(
        &recovered,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    close(&first, &present(&recovered, &mut p, v));
}

#[test]
fn in_surface_navigator_matches_bounded_preview_and_keeps_clipped_geometry() {
    let doc = document([1537, 769]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
    submit(&mut r, &doc, view([1., 0., 0., 1., -900., -400.]), true);
    assert!(r.request_canvas_preview(None).unwrap());
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    let preview = r.take_canvas_preview().unwrap().unwrap().image.unwrap();
    let size = [preview.width, preview.height];
    let (texture, target) = create_target(
        &r.device,
        size,
        wgpu::TextureFormat::Rgba8Unorm,
        "coarse native Navigator",
    );
    let mut presenter = ViewportPresenter::for_overviews(&r, wgpu::TextureFormat::Rgba8Unorm);
    for clipped in [false, true] {
        presenter.set_overviews(
            &r,
            &[OverviewPlacement {
                bounds: [0., 0., size[0] as f32, size[1] as f32],
                clip: clipped.then_some([13., 17., (size[0] - 31) as f32, (size[1] - 37) as f32]),
                work_area: [[-1000.; 2]; 4],
                outline_linear: [0.; 3],
                background_linear: [1.; 3],
                scale: 1.,
                opacity: 1.,
            }],
        );
        presenter.present_overviews(&r, &target, size).unwrap();
        let actual = crate::layer_tests::page_bytes(&r, &texture);
        for y in 0..size[1] {
            for x in 0..size[0] {
                let i = ((y * size[0] + x) * 4) as usize;
                if clipped && (!(13..size[0] - 18).contains(&x) || !(17..size[1] - 20).contains(&y))
                {
                    assert_eq!(&actual[i..i + 4], [0; 4]);
                } else {
                    for c in 0..4 {
                        assert!(
                            actual[i + c].abs_diff(preview.bytes[i + c]) <= 1,
                            "Navigator {x},{y} channel {c}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn large_document_waits_for_mip_compilation_before_reporting_canvas_ready() {
    use std::time::{Duration, Instant};
    let color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: IntegerDepth::U16,
    };
    let mut doc = layer_core::Document::new("large staged document", 4097, 1025);
    doc.color = color;
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    r.startup = Some(startup::Startup::new(&r.device).unwrap());
    let (release, wait) = mpsc::channel();
    let (entered, blocked) = mpsc::channel();
    let compiler = &r.startup.as_ref().unwrap().compiler;
    compiler.enqueue(0, move || {
        entered.send(()).map_err(|e| e.to_string())?;
        wait.recv_timeout(Duration::from_secs(20))
            .map_err(|e| e.to_string())
    });
    compiler.start();
    blocked.recv_timeout(Duration::from_secs(20)).unwrap();
    let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
    r.prepare_startup(&doc, &brush, false).unwrap();
    assert!(!r.poll_startup().unwrap().canvas_ready);
    assert!(!r.display_pipelines.as_ref().unwrap().reduce.ready());
    assert!(r.live_display.is_none() && r.composite_texture.is_none());
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !r.poll_startup().unwrap().canvas_ready {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(r.display_pipelines.as_ref().unwrap().reduce.ready());
    submit(&mut r, &doc, view([1., 0., 0., 1., 0., 0.]), true);
    assert!(r.live_display.is_some() && r.composite_texture.is_none());
}

#[test]
fn native_stroke_undo_redo_and_replaced_device_rebuild_visible_tiles_from_exact_backing() {
    use layer_engine::{
        CanvasEngine, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform, input_queue,
    };
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    };
    let mut doc = layer_core::Document::new("bounded native drawing", 1025, 513);
    doc.color = color;
    let v = view([1., 0., 0., 1., 0., 0.]);
    let renderer = || {
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        r.native_edit.as_mut().unwrap().display_dense_bytes = 0;
        r
    };
    let (mut input, consumer) = input_queue(64);
    let mut engine =
        CanvasEngine::new(renderer(), doc, consumer, v, ViewTransform::IDENTITY).unwrap();
    let flush = |engine: &mut CanvasEngine<WgpuRasterizer>| {
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            engine.render_frame().unwrap();
            if !engine.has_pending_input() && !engine.has_pending_document_edits() {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    };
    flush(&mut engine);
    let mut p = ViewportPresenter::for_surface(
        engine.backend(),
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let blank = present(engine.backend(), &mut p, v);
    for (i, phase) in [PenPhase::Down, PenPhase::Move, PenPhase::Up]
        .into_iter()
        .enumerate()
    {
        input
            .push(PenEvent {
                device_id: 1,
                sequence: i as u64 + 1,
                timestamp_ns: (i as u64 + 1) * 10_000_000,
                view_revision: 0,
                surface_position: layer_core::Point {
                    x: 245. + i as f32 * 18.,
                    y: 80.,
                },
                pressure: 0.37,
                tilt_radians: [0.; 2],
                twist_radians: 0.,
                distance: 0.,
                phase,
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            })
            .unwrap();
        flush(&mut engine);
    }
    let painted = present(engine.backend(), &mut p, v);
    assert!(painted != blank);
    let root = engine.document().layers[0].raster.clone();
    let backed = root.wait_data().unwrap();
    assert!(
        backed.tiles.len() >= 2,
        "stroke crosses native tile boundaries"
    );
    let exact: Vec<_> = backed
        .tiles
        .iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode().unwrap()))
        .collect();
    assert!(engine.undo().unwrap());
    flush(&mut engine);
    close(&blank, &present(engine.backend(), &mut p, v));
    assert!(engine.redo().unwrap());
    flush(&mut engine);
    close(&painted, &present(engine.backend(), &mut p, v));
    let retired = engine.replace_backend(renderer()).unwrap();
    retired.device.destroy();
    drop(retired);
    drop(p);
    flush(&mut engine);
    assert!(engine.backend().composite_texture.is_none());
    let mut p = ViewportPresenter::for_surface(
        engine.backend(),
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    close(&painted, &present(engine.backend(), &mut p, v));
    let restored = engine.document().layers[0].raster.wait_data().unwrap();
    for (key, bytes) in exact {
        assert_eq!(
            bytes,
            restored.tiles[&key]
                .wait_backing()
                .unwrap()
                .decode()
                .unwrap()
        );
    }
}
