use super::*;
use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace, rgb, source::*};

fn source(space: RgbSpace, codes: [u16; 4]) -> Layer {
    let mut builder = SourceBuilder::new(
        [256; 2],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: ColorProfile::Builtin(space),
            profile_assumed: false,
        },
        4 * 1024 * 1024,
    )
    .unwrap();
    let row: Vec<_> = codes
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>()
        .repeat(256);
    for _ in 0..256 {
        builder.push_row(&row).unwrap();
    }
    let mut layer = Layer::paint(LayerId(1), "profiled photo");
    layer.source = Some(Arc::new(builder.finish().unwrap()));
    layer
}
fn view() -> ViewState {
    ViewState {
        width_px: 256,
        height_px: 256,
        background_rgba_linear: [0.; 4],
        ..test_view()
    }
}
fn frame(r: &mut WgpuRasterizer, layer: &Layer) {
    r.submit(FramePacket {
        view: view(),
        document_extent: [256; 2],
        layers: std::slice::from_ref(layer),
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: true,
        composite_all: true,
        time_seconds: 0.,
    })
    .unwrap();
}
fn complete(r: &WgpuRasterizer) {
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
}
fn linear(space: RgbSpace, codes: [u16; 4], target: RgbSpace) -> [f64; 3] {
    rgb::apply(
        space.linear_transform(target),
        std::array::from_fn(|c| space.decode(f64::from(codes[c]) / 65535.)),
    )
}
fn bytes(rgb: [f64; 3], alpha: f64) -> [u8; 4] {
    let mut out = [0; 4];
    for c in 0..3 {
        out[c] = (RgbSpace::Srgb.encode(rgb[c]).clamp(0., 1.) * 255.).round() as u8;
    }
    out[3] = (alpha * 255.).round() as u8;
    out
}
fn close(actual: &[u8], expected: [u8; 4], context: &str) {
    assert!(
        actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
        "{context}: {actual:?} vs {expected:?}"
    );
}
fn texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("SDR viewing reference"),
        size: wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

// Float16 is allowed only for display. Bound its rounding independently of the
// Float32 document oracle: 0.05% relative plus 3e-6 arithmetic/conversion error.
fn float_surface_close(actual: &[u8], format: wgpu::TextureFormat, expected: [f64; 3]) {
    let half = format == wgpu::TextureFormat::Rgba16Float;
    for c in 0..4 {
        let value = if half {
            let bits = u16::from_le_bytes(actual[c * 2..c * 2 + 2].try_into().unwrap());
            let exponent = (bits >> 10) & 31;
            assert_ne!(exponent, 31, "display value must be finite");
            let mantissa = f64::from(bits & 1023);
            let magnitude = if exponent == 0 {
                mantissa * 2f64.powi(-24)
            } else {
                (1. + mantissa / 1024.) * 2f64.powi(i32::from(exponent) - 15)
            };
            if bits & 0x8000 == 0 {
                magnitude
            } else {
                -magnitude
            }
        } else {
            f64::from(f32::from_le_bytes(
                actual[c * 4..c * 4 + 4].try_into().unwrap(),
            ))
        };
        let expected = if c == 3 { 1. } else { expected[c] };
        let tolerance = 3e-6 + if half { 0.0005 * expected.abs() } else { 0. };
        assert!(
            (value - expected).abs() <= tolerance,
            "{format:?} channel {c}: {value} vs {expected}"
        );
    }
}

#[test]
fn native_export_navigator_thumbnails_and_raw_samples_keep_their_declared_color_coordinates() {
    use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource};
    for preview_space in [RgbSpace::Srgb, RgbSpace::DisplayP3] {
        for space in RgbSpace::ALL {
            for depth in [IntegerDepth::U8, IntegerDepth::U16] {
                let mut r =
                    WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
                r.configure_ui_previews(preview_space).unwrap();
                for codes in [
                    [17000, 65000, 5000, 65535],
                    [64000, 30000, 8000, 17000],
                    [45000, 32000, 28000, 1],
                ] {
                    // Retain ProPhoto numbers, including values outside the working
                    // gamut. No view path may clip in document coordinates first.
                    let layer = source(RgbSpace::ProPhoto, codes);
                    frame(&mut r, &layer);
                    let before =
                        crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap());
                    let expected = linear(RgbSpace::ProPhoto, codes, RgbSpace::Srgb);
                    let preview = linear(RgbSpace::ProPhoto, codes, preview_space);
                    let alpha = f64::from(codes[3]) / 65535.;
                    let export = r.readback_srgb_rgba8().unwrap();
                    close(&export[0..4], bytes(expected, alpha), "sRGB export");
                    assert!(r.request_canvas_preview(None).unwrap());
                    complete(&r);
                    let navigator = r.take_canvas_preview().unwrap().unwrap().image.unwrap();
                    close(&navigator.bytes[..4], bytes(preview, alpha), "Navigator");
                    r.request_thumbnail(7, layer.id).unwrap();
                    complete(&r);
                    let thumbnail = r.take_thumbnail().unwrap().unwrap();
                    let i = (16 * 32 + 16) * 4;
                    close(
                        &thumbnail.bytes[i..i + 4],
                        bytes(preview.map(|v| v * alpha + 0.855 * (1. - alpha)), 1.),
                        "retained photo thumbnail",
                    );
                    if codes[3] == 65535 {
                        r.request_filter_previews(layer_render::FilterPreviewRequest {
                            request_id: 9,
                            target: layer.id,
                            size: [200, 40],
                            extent: [256; 2],
                            view: view(),
                            layers: vec![layer.composite_snapshot()],
                            filters: vec![Arc::new(layer_core::EffectInstance::new(
                                fixture("exposure").program(),
                            ))],
                        })
                        .unwrap();
                        let image = (0..100)
                            .find_map(|_| {
                                complete(&r);
                                r.take_filter_previews()
                            })
                            .expect("filter image completes")
                            .unwrap()
                            .image;
                        let mut opaque = 0;
                        for pixel in image.bytes.chunks_exact(4).filter(|p| p[3] == 255) {
                            close(pixel, bytes(preview, 1.), "zero exposure filter preview");
                            opaque += 1;
                        }
                        assert!(opaque > 500);
                    }
                    for area in [ColorSampleArea::Point, ColorSampleArea::Average5] {
                        assert!(
                            r.request_color_sample(ColorSampleRequest {
                                request_id: 8,
                                source: ColorSampleSource::Composite,
                                position: [16, 16],
                                area,
                            })
                            .unwrap()
                        );
                        complete(&r);
                        let sample = r.take_color_sample().unwrap().unwrap();
                        let working = linear(RgbSpace::ProPhoto, codes, space);
                        for c in 0..3 {
                            assert!(
                                (f64::from(sample.rgba[c]) - working[c]).abs() < 3e-6,
                                "{space:?} raw {area:?}"
                            );
                        }
                        assert!((f64::from(sample.rgba[3]) - alpha).abs() < 2e-7);
                    }
                    assert_eq!(
                        crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap()),
                        before
                    );
                    assert!(
                        r.paint_layers[0].pages.is_empty(),
                        "viewing must not rasterize the source"
                    );
                }
            }
        }
    }
}

#[test]
fn explicit_sdr_surfaces_transform_artwork_and_ui_without_changing_document_pixels() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space,
            depth: IntegerDepth::U16,
        })
        .unwrap();
        let codes = [17000, 65000, 5000, 17000];
        let layer = source(RgbSpace::ProPhoto, codes);
        frame(&mut r, &layer);
        let before = crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap());
        for color in [
            SdrSurfaceColor::Srgb,
            SdrSurfaceColor::DisplayP3,
            SdrSurfaceColor::ExtendedLinearSrgb,
        ] {
            for format in if color == SdrSurfaceColor::ExtendedLinearSrgb {
                vec![
                    wgpu::TextureFormat::Rgba16Float,
                    wgpu::TextureFormat::Rgba32Float,
                ]
            } else {
                vec![
                    wgpu::TextureFormat::Rgba8Unorm,
                    wgpu::TextureFormat::Rgba8UnormSrgb,
                ]
            } {
                let target = texture(&r, format);
                let target_view = target.create_view(&Default::default());
                let mut presenter = ViewportPresenter::for_surface(&r, format, color).unwrap();
                let alpha = f64::from(codes[3]) / 65535.;
                let expected = linear(RgbSpace::ProPhoto, codes, color.primaries())
                    .map(|v| v * alpha + 0.94 * (1. - alpha));
                presenter
                    .present(&r, &target_view, view(), [0.; 4])
                    .unwrap();
                let actual = crate::layer_tests::page_bytes(&r, &target);
                let i = (16 * 256 + 16) * format.block_copy_size(None).unwrap() as usize;
                if color == SdrSurfaceColor::ExtendedLinearSrgb {
                    float_surface_close(&actual[i..], format, expected);
                } else {
                    close(&actual[i..i + 4], bytes(expected, 1.), "managed surface");
                }
                // Colored application surround stays sRGB-defined even on P3.
                let surround = [0.15, 0.45, 0.07, 1.];
                let expected = rgb::apply(
                    RgbSpace::Srgb.linear_transform(color.primaries()),
                    [0.15, 0.45, 0.07],
                );
                let mut outside = view();
                outside.document_to_surface[4] = 256.;
                presenter
                    .present(&r, &target_view, outside, surround)
                    .unwrap();
                let actual = crate::layer_tests::page_bytes(&r, &target);
                if color == SdrSurfaceColor::ExtendedLinearSrgb {
                    float_surface_close(&actual, format, expected);
                } else {
                    close(&actual[..4], bytes(expected, 1.), "UI surround");
                }
            }
        }
        assert!(
            ViewportPresenter::for_surface(
                &r,
                wgpu::TextureFormat::Rgba8Unorm,
                SdrSurfaceColor::ExtendedLinearSrgb
            )
            .is_err()
        );
        assert_eq!(
            crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap()),
            before
        );
    }
}

#[test]
fn native_paint_thumbnail_converts_the_same_color_as_export() {
    use layer_core::raster::*;
    for space in RgbSpace::ALL {
        let color = DocumentColor {
            space,
            depth: IntegerDepth::U16,
        };
        let codes = [51000u16, 14000, 23000, 65535];
        let raw = codes
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
            .repeat(65536);
        let mut layer = Layer::paint(LayerId(1), "native paint");
        layer.raster = RasterRevision::backed(RasterData {
            tiles: [(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [0, 0],
                },
                RasterTile::backed(TileBlob::encode(color.paint_descriptor(), &raw).unwrap()),
            )]
            .into(),
            watercolor: None,
        });
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        frame(&mut r, &layer);
        r.request_thumbnail(7, layer.id).unwrap();
        complete(&r);
        let thumbnail = r.take_thumbnail().unwrap().unwrap();
        close(
            &thumbnail.bytes[(16 * 32 + 16) * 4..][..4],
            bytes(linear(space, codes, RgbSpace::Srgb), 1.),
            "paint thumbnail",
        );
    }
}

#[test]
fn zero_coverage_export_and_navigator_return_black_without_mutating_the_artwork() {
    for native in [false, true] {
        let mut r = if native {
            WgpuRasterizer::new_native_headless(DocumentColor {
                space: RgbSpace::ProPhoto,
                depth: IntegerDepth::U16,
            })
            .unwrap()
        } else {
            WgpuRasterizer::new_headless().unwrap()
        };
        frame(&mut r, &source(RgbSpace::Srgb, [65535; 4]));
        let artwork = r.readback_srgb_rgba8().unwrap();
        // Unassociated export is undefined at zero coverage. Even if a custom
        // effect leaves hidden RGB, the output boundary must emit canonical zero.
        let pixel = if native {
            [0.7f32, -0.1, 1.5, 0.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>()
        } else {
            vec![255, 75, 132, 0]
        };
        let input = pixel.repeat(256 * 256);
        let texture = r.composite_texture.as_ref().unwrap();
        r.queue.write_texture(
            texture.as_image_copy(),
            &input,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * pixel.len() as u32),
                rows_per_image: Some(256),
            },
            texture.size(),
        );
        // Exercise the unassociated output boundary with deliberately hidden
        // RGB. Exact artwork readback must ignore the corrupted display cache.
        let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let binding = r.composite_bind_group.as_ref().unwrap().clone();
        let (tx, rx) = std::sync::mpsc::channel();
        r.submit_ui_readback(encoder, &binding, [1; 2], 42, move |image| {
            tx.send(image).unwrap();
        });
        complete(&r);
        assert!(rx.recv().unwrap().unwrap().bytes.iter().all(|v| *v == 0));
        assert_eq!(r.readback_srgb_rgba8().unwrap(), artwork);
        assert!(r.request_canvas_preview(None).unwrap());
        complete(&r);
        let preview = r.take_canvas_preview().unwrap().unwrap().image.unwrap();
        assert!(preview.bytes.iter().all(|v| *v == 0));
        assert_eq!(
            crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap()),
            input
        );
    }
}

#[test]
fn proof_view_matches_cpu_and_never_changes_artwork_or_export() {
    use layer_core::color::ProofRecipe;
    check_proof_view(&ProofRecipe::new("sRGB proof".into(), ColorProfile::Builtin(RgbSpace::Srgb)));
}

#[test]
#[ignore = "local licensed CMYK profile in LAYER_GPU_PROOF_PROFILE"]
fn proof_shadow_grid_matches_cpu_and_never_changes_artwork_or_export() {
    use layer_core::color::{ProofRecipe, RenderingIntent};
    let profile = ColorProfile::Icc(std::fs::read(std::env::var_os("LAYER_GPU_PROOF_PROFILE").unwrap()).unwrap().into());
    let mut recipe = ProofRecipe::new("CMYK saturation".into(), profile);
    recipe.conversion.intent = RenderingIntent::Saturation;
    recipe.simulate_black_ink = false;
    let lut = layer_color::ProofLut::build(RgbSpace::ProPhoto, &recipe, || false).unwrap();
    assert!(lut.dark_grid(), "this fixture exercises the refined shadow grid");
    check_proof_view(&recipe);
}

fn check_proof_view(recipe: &layer_core::color::ProofRecipe) {
    for space in RgbSpace::ALL {
        let lut = Arc::new(layer_color::ProofLut::build(space, recipe, || false).unwrap());
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let target = texture(&r, format);
            let target_view = target.create_view(&Default::default());
            for surface in [SdrSurfaceColor::Srgb, SdrSurfaceColor::DisplayP3] {
                let mut presenter = ViewportPresenter::for_surface(&r, format, surface).unwrap();
                for codes in [[63124, 2917, 23000, 0], [32768, 23111, 11300, 1],
                    [432, 893, 200, 17000], [61111, 51222, 9999, 65535], [30000; 4]] {
                    frame(&mut r, &source(space, codes));
                    let raw = crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap());
                    let exported = r.readback_srgb_rgba8().unwrap();
                    let alpha = codes[3] as f32 / 65535.;
                    let input = std::array::from_fn(|i| if i == 3 { alpha }
                        else { space.decode(codes[i] as f64 / 65535.) as f32 * alpha });
                    for (proof, warning) in [(false, false), (true, false), (true, true), (false, true), (false, false)] {
                        presenter.set_proof(&r, Some(lut.clone()), proof, warning).unwrap();
                        presenter.present(&r, &target_view, view(), [0.; 4]).unwrap();
                        let actual = crate::layer_tests::page_bytes(&r, &target);
                        let expected = lut.apply_premultiplied(input, proof, warning);
                        let expected = rgb::apply(space.linear_transform(surface.primaries()),
                            [expected[0] as f64, expected[1] as f64, expected[2] as f64])
                            .map(|v| v + 0.94 * (1. - alpha as f64));
                        let i = (16 * 256 + 16) * 4;
                        close(&actual[i..i+4], bytes(expected, 1.), "CPU/GPU proof parity");
                        assert_eq!(r.readback_srgb_rgba8().unwrap(), exported, "proof must not reach export");
                        assert_eq!(crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap()), raw);
                    }
                }
            }
        }
    }
}
