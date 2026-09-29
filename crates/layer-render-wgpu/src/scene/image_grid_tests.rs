use super::*;

#[test]
fn independent_pointwise_tiles_share_a_compute_pass() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r);
    let image = Image::new(&r, display_mips::Plan::at([PAGE_SIZE; 2], 0), "pointwise source");
    let color = [0.2_f32, 0.1, 0.05, 0.5];
    let bytes: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE).flat_map(|_| color.into_iter().flat_map(f32::to_le_bytes)).collect();
    r.queue.write_texture(image.texture.as_image_copy(), &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(PAGE_SIZE * 16), rows_per_image: Some(PAGE_SIZE) }, image.texture.size());
    let outputs: Vec<_> = [Convert::None, Convert::Encode, Convert::Decode].into_iter().map(|convert| {
        let output = scene.alloc(&r, wgpu::Color::TRANSPARENT);
        scene.draw(&r, output, image.view.clone(), None, [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32],
            [1., 1., 0., 0.], false, convert);
        (output, convert)
    }).collect();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    scene.encode_jobs(&mut r, &mut encoder).unwrap();
    assert_eq!(encoder.pass_count(), 1);
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    for (output, convert) in outputs {
        let result = crate::layer_tests::page_bytes(&r, &scene.pool[output].texture);
        let expected: [f64; 4] = std::array::from_fn(|i| {
            let value = f64::from(color[i]);
            if i == 3 { return value; }
            match convert {
                Convert::Encode => layer_core::color::RgbSpace::Srgb.encode(value * 2.) * 0.5,
                Convert::Decode => layer_core::color::RgbSpace::Srgb.decode(value * 2.) * 0.5,
                _ => value,
            }
        });
        for pixel in result.chunks_exact(16) { for (bytes, value) in pixel.chunks_exact(4).zip(expected) {
            assert!((f64::from(f32::from_le_bytes(bytes.try_into().unwrap())) - value).abs() < 2e-6);
        } }
    }
}

#[test]
fn normal_composition_converts_in_its_final_destination() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r);
    let color = [0.2_f32, 0.1, 0.05, 0.5];
    let image = Image::new(&r, display_mips::Plan::at([PAGE_SIZE; 2], 0), "normal source");
    let bytes: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE).flat_map(|_| color.into_iter().flat_map(f32::to_le_bytes)).collect();
    r.queue.write_texture(image.texture.as_image_copy(), &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(PAGE_SIZE * 16), rows_per_image: Some(PAGE_SIZE) }, image.texture.size());
    for count in [1, 2] {
        scene.begin_frame();
        let output = scene.alloc(&r, wgpu::Color::TRANSPARENT);
        for _ in 0..count {
            scene.draw(&r, output, image.view.clone(), None, [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32],
                [7., 0.7, 1., 0.], true, Convert::Encode);
        }
        let output = scene.converted(&r, output, Convert::DecodeStore);
        assert_eq!(scene.jobs.iter().filter(|j| matches!(j, Job::Draw { .. })).count(), 1,
            "normal layers and their output conversion require one destination write");
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        scene.encode_jobs(&mut r, &mut encoder).unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        let alpha = 1. - (1. - f64::from(color[3]) * 0.7).powi(count);
        let expected = [f64::from(color[0]) * 2. * alpha, f64::from(color[1]) * 2. * alpha, f64::from(color[2]) * 2. * alpha, alpha];
        let result = crate::layer_tests::page_bytes(&r, &scene.pool[output].texture);
        for pixel in result.chunks_exact(16) { for (bytes, value) in pixel.chunks_exact(4).zip(expected) {
            assert!((f64::from(f32::from_le_bytes(bytes.try_into().unwrap())) - value).abs() < 2e-6);
        } }
        scene.free(output);
    }
}

#[test]
fn complete_compute_composition_replaces_the_clear_but_partial_writes_preserve_it() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r);
    let image = Image::new(&r, display_mips::Plan::at([PAGE_SIZE; 2], 0), "composition source");
    let color = [0.2_f32, 0.1, 0.05, 0.5];
    let bytes: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE).flat_map(|_| color.into_iter().flat_map(f32::to_le_bytes)).collect();
    r.queue.write_texture(image.texture.as_image_copy(), &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(PAGE_SIZE * 16), rows_per_image: Some(PAGE_SIZE) },
        image.texture.size());
    for width in [PAGE_SIZE, PAGE_SIZE / 2] {
        scene.begin_frame();
        let output = scene.alloc(&r, wgpu::Color { r: 0.3, g: 0.4, b: 0.6, a: 1. });
        let region = PixelRect::new(0, 0, width, PAGE_SIZE);
        scene.draw(&r, output, image.view.clone(), None, [0., 0., width as f32, PAGE_SIZE as f32],
            [7., 1., 1., 0.], true, Convert::None);
        let Some(Job::Draw { clip, .. }) = scene.jobs.last_mut() else { unreachable!() };
        *clip = Some(region);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        scene.encode_jobs(&mut r, &mut encoder).unwrap();
        let passes = encoder.pass_count();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        let result = crate::layer_tests::page_bytes(&r, &scene.pool[output].texture);
        for (i, bytes) in result.chunks_exact(16).enumerate() {
            let expected = if i as u32 % PAGE_SIZE < width { [0.35, 0.3, 0.35, 1.] } else { [0.3, 0.4, 0.6, 1.] };
            for (bytes, value) in bytes.chunks_exact(4).zip(expected) {
                assert!((f32::from_le_bytes(bytes.try_into().unwrap()) - value).abs() < 1e-6);
            }
        }
        assert_eq!(passes, if width == PAGE_SIZE { 1 } else { 2 }, "width {width}");
        scene.free(output);
    }
}

#[test]
fn effect_grids_preserve_document_coordinates_and_partial_edge_centers() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let extent = [547, 319];
    let mut scene = Scene::new(&r);
    let mut program = (*crate::tests::fixture("exposure").program()).clone();
    program.kind = layer_core::EffectKind::Generator;
    program.entry = "grid_probe".into();
    program.wgsl = "fn grid_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{
        let extent=fx_extent();return vec4(fx_sample(p+vec2(7.25,0.)).r,
            fx_original(p-vec2(0.,5.5)).g,(p.x/extent.x+p.y/extent.y)*.25,1.);}".into();
    program.passes = vec![layer_core::EffectPass {
        entry: program.entry.clone(), sampling: layer_core::EffectSampling::Neighborhood { radius: 8 },
    }].into();
    let mut layer = Layer::paint(LayerId(1), "Grid probe");
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(layer_core::EffectInstance::new(Arc::new(program))));
    let center = |index: u32, origin: u32, end: u32, level: u32| {
        let start = origin + (index << level);
        (start as f32 + (start + (1 << level)).min(end) as f32) * 0.5
    };
    let grids = [(0, 0, 0), (2, 1, 3), (3, 3, 1), (1, 3, 2)];
    let cases = [(1., 0.), (131072., -65536.)].into_iter().flat_map(|(scale, offset)|
        grids.map(|(output, front, original)| (scale, offset, output, front, original)));
    for (scale, offset, output_level, front_level, original_level) in cases {
        let output = display_mips::Plan::window(extent, output_level, PixelRect::new(40, 24, extent[0], extent[1]));
        let front = display_mips::Plan::window(extent, front_level, PixelRect::new(16, 8, extent[0], extent[1]));
        let original = display_mips::Plan::at(extent, original_level);
        let upload = |plan: display_mips::Plan| {
            let image = Image::new(&r, plan, "effect grid input");
            let bytes: Vec<_> = (0..plan.size[1]).flat_map(|y| (0..plan.size[0]).flat_map(move |x| {
                [center(x, plan.bounds.min_x(), plan.bounds.max_x(), plan.level) / extent[0] as f32 * scale + offset,
                    center(y, plan.bounds.min_y(), plan.bounds.max_y(), plan.level) / extent[1] as f32 * scale + offset, 0.2, 1.]
                    .into_iter().flat_map(f32::to_le_bytes)
            })).collect();
            r.queue.write_texture(image.texture.as_image_copy(), &bytes,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(plan.size[0] * 16), rows_per_image: Some(plan.size[1]) },
                image.texture.size());
            image
        };
        let front_image = upload(front);
        let original_image = upload(original);
        for execution in [effects::Execution::Image(0), effects::Execution::Preview] {
            let result = Image::new(&r, output, "effect grid result");
            let prepared = scene.effects.prepare(&r, &[&layer], execution, 0., 0, Default::default()).unwrap();
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            scene.begin_frame();
            scene.jobs.push(Job::Effect {
                target: result.view.clone(), sources: [front_image.view.clone(), original_image.view.clone(), r.empty_view.clone()],
                data: effects::image_grid(output, front, original), prepared,
                masks: Box::new(std::array::from_fn(|_| r.empty_view.clone())),
            });
            scene.encode_jobs(&mut r, &mut encoder).unwrap();
            r.uploads.finish(&encoder);
            encoder.submit(&r.queue);
            let bytes = crate::layer_tests::page_bytes(&r, &result.texture);
            for (index, pixel) in bytes.chunks_exact(16).enumerate() {
                let [x, y] = [index as u32 % output.size[0], index as u32 / output.size[0]];
                let px = center(x, output.bounds.min_x(), output.bounds.max_x(), output.level);
                let py = center(y, output.bounds.min_y(), output.bounds.max_y(), output.level);
                let expected = [
                    (px + 7.25).clamp(center(0, front.bounds.min_x(), extent[0], front.level),
                        center(front.size[0] - 1, front.bounds.min_x(), extent[0], front.level)) / extent[0] as f32 * scale + offset,
                    (py - 5.5).clamp(center(0, 0, extent[1], original.level),
                        center(original.size[1] - 1, 0, extent[1], original.level)) / extent[1] as f32 * scale + offset,
                    (px / extent[0] as f32 + py / extent[1] as f32) * 0.25, 1.,
                ];
                let sampling_error = |level: u32, extent: u32| if level == 0 { 0. } else { (1 << level) as f32 / (16. * extent as f32) };
                let tolerance = [(2e-6 + sampling_error(front_level, extent[0])) * scale,
                    (2e-6 + sampling_error(original_level, extent[1])) * scale, 2e-6, 2e-6];
                for channel in 0..4 {
                    let actual = f32::from_le_bytes(pixel[channel * 4..channel * 4 + 4].try_into().unwrap());
                    assert!((actual - expected[channel]).abs() < tolerance[channel],
                        "scale={scale}, levels={output_level}/{front_level}/{original_level}, pixel={x}/{y}/{channel}: {actual} != {}", expected[channel]);
                }
            }
        }
    }
}
