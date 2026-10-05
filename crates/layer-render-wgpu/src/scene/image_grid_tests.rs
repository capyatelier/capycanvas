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
    let extent = [1091, 613];
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
    use layer_core::authored::*;
    use super::Image;
    let mut artwork = Artwork::new(extent).unwrap();
    let program = Arc::new(program);
    let values = layer_core::EffectInstance::new(program.clone()).values;
    let application = EffectApplication::new(program, values, extent);
    let effect = artwork.effects.insert(PortableId::random(), application).unwrap();
    let handle = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), "Grid probe")).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(handle);
    let document = layer_core::Document::from_artwork(artwork).unwrap();
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
            let prepared = scene.effects.prepare(&r, document.scene(), &[handle], execution, 0., 0, Default::default()).unwrap();
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

#[test]
fn authored_spatial_references_preserve_builtin_windows_and_orientation() {
    use layer_core::{EffectInstance, EffectValue, authored::*};
    use super::Image;
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r);
    let extent = [64, 48];
    let cases: &[(&str, &[(&str, f32)])] = &[
        ("gaussian_blur", &[("sigma", 2.75)]),
        ("motion_blur", &[("distance", 7.5), ("angle", 31.)]),
        ("denoise", &[("strength", 67.)]),
        ("vignette", &[("center_x", 23.), ("center_y", 71.), ("radius", 42.)]),
        ("film_grain", &[("amount", 37.), ("size", 2.75)]),
        ("pixel_mosaic", &[("size", 7.25)]),
        ("vhs", &[("distance", 6.75), ("lines", 47.)]),
        ("crt", &[("curvature", 3.25), ("mask", 57.), ("separation", 2.5)]),
        ("ripple", &[("distance", 7.25), ("wavelength", 23.), ("center_x", 31.)]),
    ];
    for &(id, values) in cases {
        let mut draft = EffectInstance::new(crate::tests::fixture(id).program());
        for &(key, value) in values { draft.set(key, EffectValue::Number(value)).unwrap(); }
        let render = |r: &mut WgpuRasterizer, scene: &mut Scene, orientation: u8, window: bool, dense: bool| {
            let size = match orientation {1=>[extent[1],extent[0]],3=>[38,26],4=>[extent[0]*3,extent[1]*5],_=>extent};
            let mut artwork = Artwork::new(size).unwrap();
            let mut application = EffectApplication::new(draft.program.clone(), draft.values.clone(), extent);
            let reference = application.spatial.as_mut().unwrap();
            reference.extent = [101., 79.];
            reference.mapping = Affine64(match orientation {
                1 => [0., 1., -1., 0., 38.5, -13.25],
                2 => [-1., 0., 0., 1., 77.25, 9.5],
                3 => [1., 0., 0., 1., -26.25, -1.5],
                4 => [3., 0., 0., 5., -39.75, 47.5],
                _ => [1., 0., 0., 1., -13.25, 9.5],
            });
            let effect = artwork.effects.insert(PortableId::random(), application).unwrap();
            let handle = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(effect), id)).unwrap();
            let stack = artwork.compositions.get(artwork.root).unwrap().result;
            artwork.stacks.get_mut(stack).unwrap().entries.push(handle);
            let document = layer_core::Document::from_artwork(artwork).unwrap();
            let mut input_plan = display_mips::Plan::at(if orientation==3 {extent}else{size}, 0);
            if orientation==3 {
                input_plan.doc_bounds=DocRect {min:[-13,-11],max:[51,37]};
                input_plan.support=input_plan.doc_bounds;
            }
            if dense { input_plan.size = size.map(|v| v*2); }
            let input = Image::new(r, input_plan, "spatial reference source");
            let base = |x: u32, y: u32| {
                let checker = ((x/3 + y/5) % 2) as f32;
                [0.03+x as f32*0.006, 0.07+y as f32*0.007, 0.05+checker*0.23, 1.]
            };
            let source: Vec<_> = (0..input_plan.size[1]).flat_map(|y| (0..input_plan.size[0]).flat_map(move |x| {
                let [x,y] = match orientation { 1=>[y,extent[1]-1-x], 2=>[extent[0]-1-x,y], _=>[x,y] };
                let pixel = if orientation==4 {
                    let p=[(x as f32+0.5)/3.-0.5,(y as f32+0.5)/5.-0.5];
                    let lo=p.map(|v|v.floor().max(0.) as u32);
                    let hi=std::array::from_fn::<_,2,_>(|i|(p[i].floor()+1.).max(0.).min((extent[i]-1) as f32) as u32);
                    let f=p.map(|v|v-v.floor());
                    let a=base(lo[0].min(extent[0]-1),lo[1].min(extent[1]-1));let b=base(hi[0],lo[1].min(extent[1]-1));
                    let c=base(lo[0].min(extent[0]-1),hi[1]);let d=base(hi[0],hi[1]);
                    std::array::from_fn(|i|(a[i]*(1.-f[0])+b[i]*f[0])*(1.-f[1])+(c[i]*(1.-f[0])+d[i]*f[0])*f[1])
                } else if dense {
                    let a=base(x/2,y/2);let b=base((x/2+1).min(size[0]-1),y/2);
                    let c=base(x/2,(y/2+1).min(size[1]-1));let d=base((x/2+1).min(size[0]-1),(y/2+1).min(size[1]-1));
                    let fx=(x%2) as f32*0.5;let fy=(y%2) as f32*0.5;
                    std::array::from_fn(|i| (a[i]*(1.-fx)+b[i]*fx)*(1.-fy)+(c[i]*(1.-fx)+d[i]*fx)*fy)
                } else { base(x,y) };
                pixel.into_iter().flat_map(f32::to_le_bytes)
            })).collect();
            r.queue.write_texture(input.texture.as_image_copy(), &source,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(input_plan.size[0]*16), rows_per_image: Some(input_plan.size[1]) }, input.texture.size());
            let mut previous = input;
            let original = previous.view.clone();
            let passes = draft.program.passes.len().max(1);
            for pass in 0..passes {
                let bounds = if window && pass+1 == passes { PixelRect::new(13, 11, 51, 37) } else { PixelRect::full(size) };
                let mut plan = display_mips::Plan::window(size, 0, bounds);
                if orientation==3 && pass+1<passes {plan=input_plan;}
                if dense { plan.size = plan.size.map(|v| v*2); }
                let output = Image::new(r, plan, "spatial reference result");
                let execution = if draft.program.image_boundary() { effects::Execution::Image(pass) } else { effects::Execution::Preview };
                let prepared = scene.effects.prepare(r, document.scene(), &[handle], execution, 0., 0, Default::default()).unwrap();
                let mut data = effects::image_grid(plan, previous.plan, input_plan);
                if dense {
                    data[12]+=0.25;data[13]+=0.25;data[16]+=0.25;data[17]+=0.25;data[20]+=0.25;data[21]+=0.25;
                    data[24..27].fill(0.5);
                }
                scene.begin_frame();
                scene.jobs.push(Job::Effect { target: output.view.clone(), sources: [previous.view.clone(), original.clone(), r.empty_view.clone()],
                    data, prepared, masks: Box::new(std::array::from_fn(|_| r.empty_view.clone())) });
                let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
                scene.encode_jobs(r, &mut encoder).unwrap();
                r.uploads.finish(&encoder);
                encoder.submit(&r.queue);
                previous = output;
            }
            crate::layer_tests::page_bytes(r, &previous.texture).chunks_exact(16)
                .map(|pixel| std::array::from_fn::<_,4,_>(|i| f32::from_le_bytes(pixel[i*4..i*4+4].try_into().unwrap())))
                .collect::<Vec<_>>()
        };
        let full = render(&mut r, &mut scene, 0, false, false);
        let window = render(&mut r, &mut scene, 0, true, false);
        for y in 0..26 {for x in 0..38 {for channel in 0..4 {
            assert!((window[y*38+x][channel]-full[(y+11)*64+x+13][channel]).abs()<3e-5, "{id}: window {x}/{y}/{channel}");
        }}}
        let cropped = render(&mut r, &mut scene, 3, false, false);
        for y in 0..26 {for x in 0..38 {for channel in 0..4 {
            assert!((cropped[y*38+x][channel]-full[(y+11)*64+x+13][channel]).abs()<3e-5, "{id}: crop {x}/{y}/{channel}");
        }}}
        let dense = render(&mut r, &mut scene, 0, false, true);
        for y in 16..32 {for x in 16..48 {for channel in 0..4 {
            assert!((dense[(y*2)*128+x*2][channel]-full[y*64+x][channel]).abs()<3e-5, "{id}: 2x density {x}/{y}/{channel}");
        }}}
        let resized = render(&mut r, &mut scene, 4, false, false);
        for y in 16..32 {for x in 16..48 {for channel in 0..4 {
            assert!((resized[(y*5+2)*192+x*3+1][channel]-full[y*64+x][channel]).abs()<3e-5, "{id}: anisotropic size {x}/{y}/{channel}");
        }}}
        let reflected = render(&mut r, &mut scene, 2, false, false);
        for y in 0..48 {for x in 0..64 {for channel in 0..4 {
            assert!((reflected[y*64+x][channel]-full[y*64+63-x][channel]).abs()<3e-5, "{id}: reflection {x}/{y}/{channel}");
        }}}
        let oriented = render(&mut r, &mut scene, 1, false, false);
        for y in 0..64 {for x in 0..48 {for channel in 0..4 {
            assert!((oriented[y*48+x][channel]-full[(47-x)*64+y][channel]).abs()<3e-5, "{id}: orientation {x}/{y}/{channel}");
        }}}
    }
}

#[test]
fn multipass_opacity_reads_original_finite_support_independently_of_intermediate_support() {
    use layer_core::authored::{Artwork, EffectApplication, Occurrence, OccurrenceContent, PortableId};
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r);
    let size = [80, 64];
    let allocation = DocRect { min: [-17, -15], max: [63, 49] };
    let support = DocRect { min: [-9, -7], max: [55, 41] };
    let pixel = |x: i64, y: i64| {
        let alpha = 0.4 + (y + 7) as f32 * 0.004;
        [(0.25 + (x + 9) as f32 * 0.005) * alpha, 0.13 * alpha, 0.07 * alpha, alpha]
    };
    let mut input_plan = display_mips::Plan::at(size, 0);
    input_plan.doc_bounds = allocation;
    input_plan.support = support;
    let input = Image::new(&r, input_plan, "finite original reference");
    let bytes: Vec<_> = (0..size[1]).flat_map(|y| (0..size[0]).flat_map(move |x| {
        let x = allocation.min[0] + i64::from(x);
        let y = allocation.min[1] + i64::from(y);
        let value = if x >= support.min[0] && x < support.max[0] && y >= support.min[1] && y < support.max[1] { pixel(x, y) } else { [0.; 4] };
        value.into_iter().flat_map(f32::to_le_bytes)
    })).collect();
    r.queue.write_texture(input.texture.as_image_copy(), &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * 16), rows_per_image: Some(size[1]) }, input.texture.size());
    let render = |r: &mut WgpuRasterizer, scene: &mut Scene, opacity| {
        let mut artwork = Artwork::new([64, 48]).unwrap();
        let mut effect = layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program());
        effect.set("sigma", layer_core::EffectValue::Number(2.)).unwrap();
        let application = artwork.effects.insert(PortableId::random(), EffectApplication::new(effect.program, effect.values, [64, 48])).unwrap();
        let mut occurrence = Occurrence::new(OccurrenceContent::Effect(application), "finite support opacity");
        occurrence.opacity = opacity;
        let handle = artwork.occurrences.insert(PortableId::random(), occurrence).unwrap();
        let stack = artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries.push(handle);
        let document = layer_core::Document::from_artwork(artwork).unwrap();
        let effect = document.scene().effect(handle).unwrap();
        let mut previous = input.view.clone();
        let mut result = None;
        for pass in 0..effect.program.passes.len() {
            let mut front = input_plan;
            front.support = effects::pass_input_support(effect, pass, support, 0).unwrap();
            let output = Image::new(r, input_plan, "finite support opacity result");
            let prepared = scene.effects.prepare(r, document.scene(), &[handle], effects::Execution::Image(pass), 0., 0, layer_core::BlendSpace::Linear).unwrap();
            scene.begin_frame();
            scene.jobs.push(Job::Effect { target: output.view.clone(), sources: [previous, input.view.clone(), r.empty_view.clone()],
                data: effects::image_grid(input_plan, front, input_plan), prepared,
                masks: Box::new(std::array::from_fn(|_| r.empty_view.clone())) });
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            scene.encode_jobs(r, &mut encoder).unwrap();
            r.uploads.finish(&encoder);
            encoder.submit(&r.queue);
            previous = output.view.clone();
            result = Some(output);
        }
        crate::layer_tests::page_bytes(r, &result.unwrap().texture).chunks_exact(16)
            .map(|bytes| std::array::from_fn::<_, 4, _>(|i| f32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap())))
            .collect::<Vec<_>>()
    };
    let blurred = render(&mut r, &mut scene, 1.);
    let opacity = 0.37;
    let mixed = render(&mut r, &mut scene, opacity);
    for y in 12..52 { for x in 2..78 {
        let document = [allocation.min[0] + x as i64, allocation.min[1] + y as i64];
        let original = pixel(document[0].clamp(support.min[0], support.max[0]-1), document[1].clamp(support.min[1], support.max[1]-1));
        for channel in 0..4 {
            let expected = original[channel] * (1.-opacity) + blurred[y*80+x][channel] * opacity;
            assert!((mixed[y*80+x][channel]-expected).abs()<2e-6, "document={document:?} channel={channel}: {} != {expected}", mixed[y*80+x][channel]);
        }
    }}
}

#[test]
fn encoded_effect_jobs_retain_distinct_spatial_mappings_and_parameters_before_submission() {
    use layer_core::authored::{Affine64, Artwork, EffectApplication, Occurrence, OccurrenceContent, PortableId};
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene = Scene::new(&r);
    let size = [32, 24];
    let plan = display_mips::Plan::at(size, 0);
    let input = Image::new(&r, plan, "immutable effect reference");
    let bytes: Vec<_> = (0..size[0]*size[1]).flat_map(|_| [0.7_f32, 0.4, 0.2, 1.].into_iter().flat_map(f32::to_le_bytes)).collect();
    r.queue.write_texture(input.texture.as_image_copy(), &bytes,
        wgpu::TexelCopyBufferLayout {offset:0, bytes_per_row:Some(size[0]*16), rows_per_image:Some(size[1])}, input.texture.size());
    let mut effect = layer_core::EffectInstance::new(crate::tests::fixture("vignette").program());
    effect.set("center_x", layer_core::EffectValue::Number(23.)).unwrap();
    effect.set("center_y", layer_core::EffectValue::Number(71.)).unwrap();
    effect.set("radius", layer_core::EffectValue::Number(42.)).unwrap();
    let mut artwork = Artwork::new(size).unwrap();
    let application = artwork.effects.insert(PortableId::random(), EffectApplication::new(effect.program, effect.values, size)).unwrap();
    let handle = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(application), "immutable spatial reference")).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(handle);
    let first = layer_core::Document::from_artwork(artwork.clone()).unwrap();
    artwork.effects.get_mut(application).unwrap().spatial.as_mut().unwrap().mapping = Affine64([0., -1., 1., 0., 3.25, 29.5]);
    let second = layer_core::Document::from_artwork(artwork.clone()).unwrap();
    let application = artwork.effects.get_mut(application).unwrap();
    let radius = application.program.parameters.iter().position(|parameter| parameter.key.as_ref()=="radius").unwrap();
    application.values[radius] = layer_core::EffectValue::Number(63.);
    let third = layer_core::Document::from_artwork(artwork).unwrap();
    let render = |r:&mut WgpuRasterizer, scene:&mut Scene, documents:&[&layer_core::Document]| {
        scene.begin_frame();
        let outputs: Vec<_> = documents.iter().map(|document| {
            let output = Image::new(r, plan, "immutable spatial result");
            let prepared = scene.effects.prepare(r, document.scene(), &[handle], effects::Execution::Preview, 0., 0, layer_core::BlendSpace::Linear).unwrap();
            scene.jobs.push(Job::Effect {target:output.view.clone(), sources:[input.view.clone(), input.view.clone(), r.empty_view.clone()],
                data:effects::image_grid(plan, plan, plan), prepared, masks:Box::new(std::array::from_fn(|_| r.empty_view.clone()))});
            output
        }).collect();
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        scene.encode_jobs(r, &mut encoder).unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        outputs.into_iter().map(|output| crate::layer_tests::page_bytes(r, &output.texture)).collect::<Vec<_>>()
    };
    let documents = [&first, &second, &third];
    let expected: Vec<_> = documents.iter().map(|document| render(&mut r, &mut scene, &[*document]).pop().unwrap()).collect();
    assert_ne!(expected[0], expected[1]);
    assert_ne!(expected[1], expected[2]);
    assert_eq!(render(&mut r, &mut scene, &documents), expected);
}
