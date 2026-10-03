use super::*;
use wgpu::util::DeviceExt;
use layer_core::{ArtworkSample, ArtworkSampleRequest, ArtworkSource};

pub(crate) struct SamplePipeline {
    layout: wgpu::BindGroupLayout,
    measure: wgpu::ComputePipeline,
    accumulate: wgpu::ComputePipeline,
}
impl SamplePipeline {
    fn new(device: &PipelineDevice) -> Arc<Self> {
        device.sample_pipeline.get_or_init(|| {
            let layout = crate::bindings::layout(device, "artwork sample", &[
                crate::bindings::texture(0, wgpu::ShaderStages::COMPUTE, false),
                crate::bindings::buffer(1, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Uniform, false, NonZeroU64::new(32)),
                crate::bindings::buffer(2, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Storage { read_only: false }, false, NonZeroU64::new(32)),
            ]);
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("artwork sample"), source: wgpu::ShaderSource::Wgsl(concat!(include_str!("../float_number.wgsl"),"\n",include_str!("sample.wgsl")).into()),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("artwork sample"), bind_group_layouts: &[Some(&layout)], immediate_size: 0,
            });
            let pipeline = |entry| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("artwork sample"), layout: Some(&pipeline_layout), module: &shader,
                entry_point: Some(entry), compilation_options: Default::default(), cache: None,
            });
            Arc::new(Self { layout, measure: pipeline("measure"), accumulate: pipeline("accumulate") })
        }).clone()
    }

    fn encode(&self, device: &PipelineDevice, texture: &wgpu::Texture, encoder: &mut submission::CommandEncoder,
        origin: [u32; 2], center: [u32; 2], width: u32) -> wgpu::Buffer {
        let words = [origin[0], origin[1], center[0], center[1], texture.width(), texture.height(), width, 0];
        let area = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("artwork sample area"), contents: &words.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("artwork sample summary"), size: 32,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false,
        });
        let view = texture.create_view(&Default::default());
        let group = crate::bindings::group(device, "artwork sample", &self.layout, [
            wgpu::BindingResource::TextureView(&view), area.as_entire_binding(), output.as_entire_binding(),
        ]);
        for pipeline in [&self.measure, &self.accumulate] {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);pass.set_bind_group(0, &group, &[]);pass.dispatch_workgroups(1, 1, 1);
        }
        output
    }
}

impl SnapshotGpu {
    pub fn artwork_source_project(request: &layer_core::ArtworkQuery) -> Result<Project, String> {
        let (mut document, _) = Self::artwork_document(request)?;
        discard_hidden_backing(&mut document);
        Ok(Project { document })
    }
    fn artwork_document(request: &layer_core::ArtworkQuery) -> Result<(layer_core::Document, scene::Output), String> {
        request.validate()?;
        let mut document = (*request.document).clone();
        let output = match &request.source {
            ArtworkSource::Visible => scene::Output::Artwork(None),
            ArtworkSource::Reference => {document.layers = document.reference_snapshot();scene::Output::Artwork(None)}
            ArtworkSource::EffectBaseline(original) => {
                *document.layers.iter_mut().find(|layer| layer.id == original.id).unwrap() = original.composite_snapshot();
                scene::Output::Artwork(None)
            }
            ArtworkSource::EffectInput(id) | ArtworkSource::EffectChannels(id) => {
                let index = document.layers.iter().position(|layer| layer.id == *id).unwrap();
                let mut included = vec![false; document.layers.len()];
                for i in layer_core::composite_input_layers(&document.layers, index) { included[i] = true; }
                for (i, layer) in document.layers.iter_mut().enumerate() {
                    if !included[i] && layer.kind != LayerKind::Group { layer.visible = false; }
                }
                if matches!(request.source, ArtworkSource::EffectChannels(_)) { scene::Output::EffectChannels(*id) }
                else { scene::Output::EffectInput(*id) }
            }
            ArtworkSource::LayerContent(id) => {
                for layer in &mut document.layers { layer.visible = layer.id == *id || layer.kind == LayerKind::Group;layer.mask = None; }
                scene::Output::LayerContent(*id)
            }
        };
        Ok((document, output))
    }
    pub(super) async fn artwork_capture(&self, request: &layer_core::ArtworkQuery, control: CaptureControl) -> Result<(SnapshotRenderer, scene::Output), String> {
        let (mut document, output) = Self::artwork_document(request)?;
        let identity = Arc::new(document.clone());
        if let ArtworkSource::EffectChannels(id) = request.source {
            let layer = document.layers.iter_mut().find(|layer| layer.id == id).unwrap();
            layer.opacity = 1.; layer.mask = None; layer.properties.blend = layer_core::LayerBlend::Normal;
            let effect = Arc::make_mut(layer.effect.as_mut().unwrap());
            let program = Arc::make_mut(&mut effect.program); program.entry = format!("{}_channels", program.entry).into();
        }
        let background = document.layers.iter().find(|layer| layer.kind == LayerKind::Background)
            .map(|layer| layer.properties.paper_color.unwrap_or(layer_core::color::RgbColor::WHITE).linear_in(document.color.space))
            .transpose()?.unwrap_or([0.; 4]);
        discard_hidden_backing(&mut document);
        #[cfg(target_arch = "wasm32")]
        if let Some(waiter) = &self.analysis_backing_waiter { waiter(Arc::new(document.clone()), control.clone()).await?; }
        let mut snapshot = self.capture(Project { document }, background, request.time, control.clone()).map_err(|e| e.to_string())?;
        snapshot.document = identity;
        snapshot.planned_pixel_bytes = 256 * 1024 * 1024;
        for (id, phase) in request.effect_times.iter().copied() {
            if let Some(effect) = snapshot.layers.iter().find(|layer| layer.id == id).and_then(|layer| layer.effect.as_ref()) {
                snapshot.renderer.effect_clocks.insert(id, (effect.program.id.clone(), layer_core::EffectClock::at(effect, request.time, phase)));
            }
        }
        Ok((snapshot, output))
    }

    pub async fn artwork_sample(&self, request: ArtworkSampleRequest, control: CaptureControl) -> Result<ArtworkSample, String> {
        control.check().map_err(|e| e.to_string())?;
        request.validate()?;
        let extent = [request.document.width, request.document.height];
        if request.position.iter().zip(extent).any(|(p, size)| *p < 0. || *p >= size as f32) { return Ok(ArtworkSample::Outside); }
        let center = request.position.map(|v| v.floor() as u32);
        let radius = request.width / 2;
        let origin = center.map(|v| v.saturating_sub(radius));
        let size = std::array::from_fn::<_, 2, _>(|i| (center[i] + radius + 1).min(extent[i]) - origin[i]);
        let (mut snapshot, output) = self.artwork_capture(&request.query, control.clone()).await?;
        snapshot.prepare_effect_analysis_async(output).await?;
        let pipeline = SamplePipeline::new(&self.device);
        let summary = snapshot.with_region_gpu([origin[0], origin[1], size[0], size[1]], 64,
            |r, packet, region, encoder| {
                let (texture, _) = create_color_target(&r.device, size, "artwork sample footprint");
                let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
                let result = scene.capture_region(r, packet, &texture, region, output, encoder);
                r.scene = Some(scene);
                result?;
                Ok(pipeline.encode(&r.device, &texture, encoder, origin, center, request.width))
            }).map_err(|e| e.to_string())?;
        let bytes = crate::local_tone::read_buffer_async(&self.device, &self.queue, &summary).await?;
        control.check().map_err(|e| e.to_string())?;
        let words: [u32; 8] = std::array::from_fn(|i| u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap()));
        if words[3] != 0 { return Err("The sample contains invalid color values".into()); }
        let maximum = f64::from(f32::from_bits(words[0]));
        let alpha = f64::from(f32::from_bits(words[1])) * f64::from(f32::from_bits(words[7]));
        if alpha == 0. { return Ok(ArtworkSample::Empty); }
        let rgba = [
            (f64::from(f32::from_bits(words[4])) * maximum / alpha) as f32,
            (f64::from(f32::from_bits(words[5])) * maximum / alpha) as f32,
            (f64::from(f32::from_bits(words[6])) * maximum / alpha) as f32,
            (alpha / f64::from(words[2])) as f32,
        ];
        if !rgba.into_iter().all(f32::is_finite) { return Err("The sampled color is too large to represent".into()); }
        Ok(ArtworkSample::Color(rgba))
    }
}
