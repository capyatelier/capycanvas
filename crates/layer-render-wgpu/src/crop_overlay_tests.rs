use super::*;

fn target(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("crop shield"),
        size: wgpu::Extent3d { width: 128, height: 128, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

#[test]
fn the_crop_shield_dims_outside_the_crop_and_shows_added_canvas_as_transparency() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, &[Layer::paint(LayerId(1), "artwork")], &[dab([0.8, 0.1, 0.2, 1.])], &[batch(1)], true);
    let camera = ViewState { document_to_surface: [0.5, 0., 0., 0.5, 32., 32.], ..view() };
    let surround = [0.2, 0.3, 0.4, 1.];
    let texture = target(&r, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut presenter = crate::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    let mut present = |r: &WgpuRasterizer| {
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
    submit(&mut r, &[Layer::paint(LayerId(1), "artwork")], &[dab([0.8, 0.6, 0.2, 1.])], &[batch(1)], true);
    let camera = ViewState { document_to_surface: [0.5, 0., 0., 0.5, 32., 32.], ..view() };
    let crop = Rect { min: Point { x: -32., y: -32. }, max: Point { x: 64., y: 64. } };
    let unit = layer_core::Affine([crop.max.x - crop.min.x, 0., 0., crop.max.y - crop.min.y, crop.min.x, crop.min.y]);
    let dimmed = |r: &mut WgpuRasterizer, format| {
        let texture = target(r, format);
        let mut presenter = crate::ViewportPresenter::for_surface(r, format, crate::SdrSurfaceColor::Srgb).unwrap();
        let mut present = |r: &WgpuRasterizer| {
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
