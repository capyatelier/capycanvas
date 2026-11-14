use super::*;

fn target(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    crate::create_target(&r.device, [128, 128], format, "crop shield").0
}

#[test]
fn the_crop_shield_dims_outside_the_crop_and_shows_added_canvas_as_transparency() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document([128; 2], "artwork");
    crate::test_support::submit_settled(&mut r, FramePacket {dabs:&[dab([0.8, 0.1, 0.2, 1.])],dab_batches:&[batch(placement::target(&document))],reset_layers:true,..packet(document.scene(),[128;2])});
    let camera = ViewState { document_to_surface: [0.5, 0., 0., 0.5, 32., 32.], ..view() };
    let surround = [0.2, 0.3, 0.4, 1.];
    let texture = target(&r, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut presenter = crate::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    let mut present = |r: &WgpuRasterizer| {
        crate::test_support::prepare_lit_presenter(r,&mut presenter);
        presenter.present(r, &texture.create_view(&Default::default()), camera, surround).unwrap();
        page_bytes(r, &texture)
    };
    let baseline = present(&r);
    let crop = Rect { min: Point { x: -32., y: -32. }, max: Point { x: 64., y: 64. } };
    let unit = layer_core::Affine([crop.max.x - crop.min.x, 0., 0., crop.max.y - crop.min.y, crop.min.x, crop.min.y]);
    r.set_crop_overlay(Some(layer_render::CropOverlay { to_crop: unit.inverse().unwrap(), dim: 0.5 }));
    let shielded = present(&r);
    let at = |pixels: &[u8], [x, y]: [usize; 2]| -> [u8; 4] { pixels[(y * 128 + x) * 4..][..4].try_into().unwrap() };
    let inside = [50, 50];
    assert_eq!(at(&shielded, inside), at(&baseline, inside), "the crop itself is untouched");
    let dimmed = [80, 80];
    let (before, after) = (at(&baseline, dimmed), at(&shielded, dimmed));
    for c in 0..3 {
        assert!(after[c] < before[c] || before[c] == 0, "outside the crop is dimmed: {before:?} {after:?}");
    }
    let added = at(&shielded, [20, 20]);
    assert!(added[0] == added[1] && added[1] == added[2] && added[0] > 200, "added canvas is a light checkerboard: {added:?}");
    assert_ne!(added, at(&baseline, [20, 20]));
    let beyond = [120, 20];
    assert_eq!(at(&shielded, beyond), at(&baseline, beyond), "the surround outside both is unchanged");
    r.set_crop_overlay(None);
    assert_eq!(present(&r), baseline, "no crop, no shield");
}

#[test]
fn the_crop_shield_darkens_encoding_and_linear_surfaces_alike() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document([128; 2], "artwork");
    crate::test_support::submit_settled(&mut r, FramePacket {dabs:&[dab([0.8, 0.6, 0.2, 1.])],dab_batches:&[batch(placement::target(&document))],reset_layers:true,..packet(document.scene(),[128;2])});
    let camera = ViewState { document_to_surface: [0.5, 0., 0., 0.5, 32., 32.], ..view() };
    let crop = Rect { min: Point { x: -32., y: -32. }, max: Point { x: 64., y: 64. } };
    let unit = layer_core::Affine([crop.max.x - crop.min.x, 0., 0., crop.max.y - crop.min.y, crop.min.x, crop.min.y]);
    let dimmed = |r: &mut WgpuRasterizer, format| {
        let texture = target(r, format);
        let mut presenter = crate::ViewportPresenter::for_surface(r, format, crate::SdrSurfaceColor::Srgb).unwrap();
        let mut present = |r: &WgpuRasterizer| {
            pollster::block_on(presenter.prepare(r)).unwrap();
            presenter.present(r, &texture.create_view(&Default::default()), camera, [0.2, 0.3, 0.4, 1.]).unwrap();
            let pixels = page_bytes(r, &texture);
            <[u8; 4]>::try_from(&pixels[(80 * 128 + 80) * 4..][..4]).unwrap()
        };
        r.set_crop_overlay(None);
        let before = present(r);
        r.set_crop_overlay(Some(layer_render::CropOverlay { to_crop: unit.inverse().unwrap(), dim: 0.8 }));
        (before, present(r))
    };
    let linear = |v: u8| layer_core::color::srgb_decode(f32::from(v) / 255.);
    for format in [wgpu::TextureFormat::Rgba8UnormSrgb, wgpu::TextureFormat::Rgba8Unorm] {
        let (before, after) = dimmed(&mut r, format);
        for c in 0..3 {
            let (light, expected) = (linear(after[c]), linear(before[c]) * 0.2);
            assert!((light - expected).abs() <= 0.006, "{format:?} keeps a fifth of the light: {before:?} {after:?}");
        }
    }
}

#[test]
fn paper_relief_shades_only_the_canvas_and_leaves_no_trace() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document([128; 2], "artwork");
    crate::test_support::submit_settled(&mut r, FramePacket {dabs:&[dab([0.8, 0.6, 0.2, 1.])],dab_batches:&[batch(placement::target(&document))],reset_layers:true,..packet(document.scene(),[128;2])});
    let camera = ViewState { document_to_surface: [0.5, 0., 0., 0.5, 32., 32.], ..view() };
    let texture = target(&r, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut presenter = crate::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    let mut present = |r: &WgpuRasterizer| {
        crate::test_support::prepare_lit_presenter(r,&mut presenter);
        presenter.present(r, &texture.create_view(&Default::default()), camera, [0.2, 0.3, 0.4, 1.]).unwrap();
        page_bytes(r, &texture)
    };
    let baseline = present(&r);
    let paper = layer_core::paper::PaperBinding { surface: std::sync::Arc::new(layer_core::paper::PaperSurface {
        texture: layer_core::paper::built_in_paper("rough").unwrap().texture.clone().unwrap(),
        mapping: layer_core::paper::reference([128; 2], [300.; 2]).mapping,
    }), target_offset: [0; 2] };
    r.set_paper_overlay(Some(layer_render::PaperOverlay { paper: paper.clone(), opacity: 1., moving: false }));
    assert!(!r.paper_overlay_ready(&paper));
    let shaded = present(&r);
    assert!(r.paper_overlay_ready(&paper));
    let pixel = |pixels: &[u8], x: usize, y: usize| -> [u8; 4] { pixels[(y * 128 + x) * 4..][..4].try_into().unwrap() };
    let changed = (34..94).flat_map(|y| (34..94).map(move |x| (x, y))).filter(|&(x, y)| pixel(&shaded, x, y) != pixel(&baseline, x, y)).count();
    assert!(changed > 1200, "the relief shades the canvas: {changed} pixels");
    for (x, y) in [(10, 10), (120, 120), (10, 120)] {
        assert_eq!(pixel(&shaded, x, y), pixel(&baseline, x, y), "the surround is unshaded");
    }
    r.set_paper_overlay(Some(layer_render::PaperOverlay { paper: paper.clone(), opacity: 0.5, moving: false }));
    assert!(r.paper_overlay_ready(&paper), "fading keeps the prepared binding");
    let mut replacement = paper.clone();
    replacement.target_offset = [1, 0];
    r.set_paper_overlay(Some(layer_render::PaperOverlay { paper: replacement.clone(), opacity: 1., moving: false }));
    assert!(!r.paper_overlay_ready(&replacement));
    assert!(!r.paper_overlay_ready(&paper));
    present(&r);
    assert!(r.paper_overlay_ready(&replacement));
    r.set_paper_overlay(None);
    assert!(!r.paper_overlay_ready(&paper));
    assert_eq!(present(&r), baseline, "no overlay, no relief");
}

#[test]
fn staged_paper_relief_follows_a_new_scale_at_once_and_reports_only_its_own_bake() {
    use std::time::{Duration, Instant};
    let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document([128; 2], "artwork");
    let mut r = crate::test_support::staged_renderer(&reference, document.composition().color);
    r.prepare_startup(&document, &layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), false).unwrap();
    crate::test_support::wait_startup(&mut r, Instant::now() + Duration::from_secs(60), |progress| progress.canvas_ready, format_args!("canvas"));
    crate::test_support::submit_settled(&mut r, FramePacket {reset_layers:true,..packet(document.scene(),[128;2])});
    let camera = ViewState { document_to_surface: [0.5, 0., 0., 0.5, 32., 32.], ..view() };
    let texture = target(&r, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut presenter = crate::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    pollster::block_on(presenter.prepare(&r)).unwrap();
    let mut present = |r: &WgpuRasterizer| {
        crate::test_support::prepare_lit_presenter(r,&mut presenter);
        presenter.present(r, &texture.create_view(&Default::default()), camera, [0.2, 0.3, 0.4, 1.]).unwrap();
        (page_bytes(r, &texture), presenter.paper_lighting_drawn())
    };
    let baseline = present(&r).0;
    let paper = |scale: f64| layer_core::paper::PaperBinding { surface: std::sync::Arc::new(layer_core::paper::PaperSurface {
        texture: layer_core::paper::built_in_paper("rough").unwrap().texture.clone().unwrap(),
        mapping: layer_core::paper::with_scale(&layer_core::paper::reference([128; 2], [300.; 2]), scale, [300.; 2]).mapping,
    }), target_offset: [0; 2] };
    let shading = |pixels: &[u8]| (34..94).flat_map(|y| (34..94).map(move |x| (y * 128 + x) * 4))
        .map(|i| f64::from(pixels[i]) - f64::from(baseline[i])).collect::<Vec<_>>();
    fn settle(r: &mut WgpuRasterizer, paper: &layer_core::paper::PaperBinding, present: &mut impl FnMut(&WgpuRasterizer) -> (Vec<u8>, bool)) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            r.poll_startup().unwrap();
            let (pixels, lit) = present(r);
            if r.paper_overlay_ready(paper) && lit { return pixels; }
            assert!(Instant::now() < deadline, "the slope pipelines and bake prepare on demand");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let (real, doubled) = (paper(1.), paper(2.));
    r.set_paper_overlay(Some(layer_render::PaperOverlay { paper: real.clone(), opacity: 1., moving: false }));
    let real_size = shading(&settle(&mut r, &real, &mut present));
    assert!(real_size.iter().any(|v| *v != 0.), "the relief shades the canvas");
    r.set_paper_overlay(Some(layer_render::PaperOverlay { paper: doubled.clone(), opacity: 1., moving: true }));
    let stretched = shading(&present(&r).0);
    assert_ne!(stretched, real_size, "the relief follows the new scale before its bake");
    let held = Instant::now() + Duration::from_millis(500);
    while Instant::now() < held {
        r.poll_startup().unwrap();
        present(&r);
        assert!(!r.paper_overlay_ready(&doubled), "a moving Scale stretches the bake it has instead of baking");
    }
    r.set_paper_overlay(Some(layer_render::PaperOverlay { paper: doubled.clone(), opacity: 1., moving: false }));
    let exact = shading(&settle(&mut r, &doubled, &mut present));
    let correlation = |a: &[f64], b: &[f64]| {
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let (ma, mb) = (mean(a), mean(b));
        let covariance: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
        covariance / (a.iter().map(|x| (x - ma).powi(2)).sum::<f64>() * b.iter().map(|y| (y - mb).powi(2)).sum::<f64>()).sqrt()
    };
    let (similar, different) = (correlation(&stretched, &exact), correlation(&real_size, &exact));
    assert!(similar > 0.5 && similar > different + 0.2, "the stretched relief resembles the new scale: {similar:.2}, against {different:.2}");
}
