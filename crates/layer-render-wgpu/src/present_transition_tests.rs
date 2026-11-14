use super::*;

#[test]
fn held_capture_blends_and_releases_its_device_charge_after_completion() {
    let renderer=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let device=&renderer.device;
    let format=wgpu::TextureFormat::Rgba8Unorm;
    let layout=crate::bindings::layout(device,"capture test",&[
        crate::bindings::buffer(0,wgpu::ShaderStages::FRAGMENT,wgpu::BufferBindingType::Uniform,true,wgpu::BufferSize::new(16))]);
    let uniform=device.create_buffer(&wgpu::BufferDescriptor {label:None,size:16,usage:wgpu::BufferUsages::UNIFORM,mapped_at_creation:false});
    let group=device.create_bind_group(&wgpu::BindGroupDescriptor {label:None,layout:&layout,entries:&[
        wgpu::BindGroupEntry {binding:0,resource:uniform.as_entire_binding()}]});
    let pipeline_layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {label:None,bind_group_layouts:&[Some(&layout)],immediate_size:0});
    let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor {label:None,source:wgpu::ShaderSource::Wgsl(r#"
        @vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32> {
            let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));return vec4(p[i],0.,1.);
        }
        @fragment fn fs()->@location(0) vec4<f32> {return vec4(1.,0.,0.,1.);}
    "#.into())});
    let pipeline=crate::present::surface_pipeline(device,"capture test",&pipeline_layout,&shader,["vs","fs"],None,format,None);
    pollster::block_on(renderer.compilation.prepare_all([&pipeline])).unwrap();
    let mut encoder=device.create_command_encoder(&Default::default());
    let view=HeldCanvas::texture(device,[8,8],format);
    HeldCanvas::capture(&mut encoder,&view,|pass| {pass.set_pipeline(&pipeline);pass.set_bind_group(0,&group,&[0]);pass.draw(0..3,0..1);});
    let held=HeldCanvas::new(device,[8,8],format,SdrSurfaceColor::Srgb,Some(view),None);
    held.retain_until_complete(device,&encoder);
    let bytes=crate::texture_bytes(held.canvas.as_ref().unwrap().texture());
    assert_eq!(device.transient_display.used(),bytes);
    assert!(held.compatible(device,[8,8],format,SdrSurfaceColor::Srgb));
    assert!(!held.compatible(device,[9,8],format,SdrSurfaceColor::Srgb));
    assert!(!held.compatible(device,[8,8],wgpu::TextureFormat::Bgra8Unorm,SdrSurfaceColor::Srgb));
    assert!(!held.compatible(device,[8,8],format,SdrSurfaceColor::WindowsScrgb));
    assert!(Arc::ptr_eq(&device.transient_display,&device.for_recipe().transient_display));
    let recipes=TransitionRecipes::new(device,format);
    pollster::block_on(renderer.compilation.prepare_all([&recipes.pipeline])).unwrap();
    let mut draw=TransitionDraw::new(device,&recipes,&held);
    let mut uploads=Uploads::new(device,1024);
    renderer.queue.submit([encoder.finish()]);
    for (alpha,expected) in [(1.,[255,0,0,255]),(0.125,[32,0,223,255]),(0.,[0,0,255,255])] {
        let (texture,target)=crate::create_target(device,[8,8],format,"fade test");
        let mut encoder=device.create_command_encoder(&Default::default());
        draw.update(&mut uploads,&mut encoder,layer_render::CanvasFade {drawing:Some(alpha),navigator:None}).unwrap();
        {
            let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {label:None,
                color_attachments:&[Some(wgpu::RenderPassColorAttachment {view:&target,depth_slice:None,resolve_target:None,
                    ops:wgpu::Operations {load:wgpu::LoadOp::Clear(wgpu::Color {r:0.,g:0.,b:1.,a:1.}),store:wgpu::StoreOp::Store}})],
                depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None,multiview_mask:None});
            draw.draw(&recipes,&mut pass,0);
        }
        held.retain_until_complete(device,&encoder);uploads.finish(&encoder);renderer.queue.submit([encoder.finish()]);
        let pixels=crate::layer_tests::page_bytes(&renderer,&texture);
        assert!(pixels.chunks_exact(4).all(|pixel|pixel.iter().zip(expected).all(|(actual,expected)|(*actual as i32-expected).abs()<=1)));
    }
    let encoder=device.create_command_encoder(&Default::default());
    held.retain_until_complete(device,&encoder);
    drop(held);drop(draw);
    assert_eq!(device.transient_display.used(),bytes);
    renderer.queue.submit([encoder.finish()]);
    device.wait_queue().unwrap();
    assert_eq!(device.transient_display.used(),0);
    pollster::block_on(renderer.compilation.prepare_all([&pipeline])).unwrap();
    let mut encoder=device.create_command_encoder(&Default::default());
    let view=HeldCanvas::texture(device,[8,8],format);
    HeldCanvas::capture(&mut encoder,&view,|pass| {pass.set_pipeline(&pipeline);pass.set_bind_group(0,&group,&[0]);pass.draw(0..3,0..1);});
    let held=HeldCanvas::new(device,[8,8],format,SdrSurfaceColor::Srgb,Some(view),None);
    held.retain_until_complete(device,&encoder);
    renderer.queue.submit([encoder.finish()]);
    device.wait_queue().unwrap();
    device.destroy();
    assert_eq!(device.transient_display.used(),bytes);
    drop(held);
    assert_eq!(device.transient_display.used(),0);
}
