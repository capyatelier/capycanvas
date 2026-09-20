//! Shared Float32 local-Laplacian compute engine. Hosts own idle scheduling and
//! cancellation; the output is immutable and may be retained across edits.
use crate::deferred::Deferred;
use crate::{PipelineDevice, submission::CommandEncoder};
use layer_core::color::hdr::LocalToneGuide;
use layer_core::color::{RgbSpace, hdr::LOCAL_GUIDE_EDGE};
use std::sync::Arc;

/// A completed document-space illumination guide on its originating device.
/// Presentation shares this buffer; it never uploads or reads image pixels.
#[derive(Clone, Debug)]
pub struct GpuToneGuide {
    pub extent: [u32; 2],
    pub document_extent: [u32; 2],
    pub space: RgbSpace,
    pub peak: f32,
    pub(crate) device: wgpu::Device,
    pub(crate) buffer: wgpu::Buffer,
}
impl GpuToneGuide {
    pub fn byte_len(&self) -> u64 {
        self.buffer.size()
    }

    /// CPU codecs can reuse the same analysis by downloading only the bounded
    /// guide. Interactive presentation never calls this method.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download(&self, queue: &wgpu::Queue) -> Result<LocalToneGuide, String> {
        pollster::block_on(self.download_async(queue))
    }

    pub async fn download_async(&self, queue: &wgpu::Queue) -> Result<LocalToneGuide, String> {
        let bytes = read_buffer_async(&self.device, queue, &self.buffer).await?;
        let dimensions: [u32; 4] = std::array::from_fn(|i| {
            u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        });
        if dimensions
            != [
                self.extent[0],
                self.extent[1],
                self.document_extent[0],
                self.document_extent[1],
            ]
        {
            return Err("Invalid GPU illumination guide geometry".into());
        }
        let samples = bytes[16..]
            .chunks_exact(16)
            .map(|p| {
                std::array::from_fn(|i| f32::from_ne_bytes(p[i * 4..i * 4 + 4].try_into().unwrap()))
            })
            .collect();
        Ok(LocalToneGuide {
            extent: self.extent,
            document_extent: self.document_extent,
            samples,
            peak: self.peak,
        })
    }
}

const ENTRIES: [&str; 9] = [
    "reduce_source",
    "normalize",
    "range_reduce",
    "downsample",
    "remap",
    "accumulate",
    "reconstruct",
    "finish",
    "init_detail",
];
pub(crate) struct Pipelines {
    layout: wgpu::BindGroupLayout,
    pipelines: Vec<Deferred<wgpu::ComputePipeline>>,
    #[cfg(target_arch = "wasm32")]
    ready: std::cell::OnceCell<
        futures_util::future::Shared<
            futures_util::future::LocalBoxFuture<'static, Result<(), String>>,
        >,
    >,
}
impl Pipelines {
    fn new(device: &PipelineDevice) -> Self {
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        for binding in 1..=4 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: binding != 4,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 5,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("local tone compute"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("local tone compute"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("local tone compute"),
            source: wgpu::ShaderSource::Wgsl(include_str!("local_tone.wgsl").into()),
        });
        let pipelines = ENTRIES
            .iter()
            .map(|entry| {
                let device = device.clone();
                let pipeline_layout = pipeline_layout.clone();
                let module = module.clone();
                Deferred::pipeline(move |mode| {
                    mode.compute(
                        &device,
                        &wgpu::ComputePipelineDescriptor {
                            label: Some(entry),
                            layout: Some(&pipeline_layout),
                            module: &module,
                            entry_point: Some(entry),
                            compilation_options: Default::default(),
                            cache: None,
                        },
                    )
                })
            })
            .collect();
        Self {
            layout,
            pipelines,
            #[cfg(target_arch = "wasm32")]
            ready: Default::default(),
        }
    }

    /// Concurrent proof and export captures await the same compilation. Browser
    /// drivers compile asynchronously; no synchronous shader build on input owner.
    async fn prepare(&self) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            use futures_util::FutureExt;
            self.ready
                .get_or_init(|| {
                    let pipelines = self.pipelines.clone();
                    async move {
                        for pipeline in pipelines {
                            pipeline.compile_async().await?;
                        }
                        Ok(())
                    }
                    .boxed_local()
                    .shared()
                })
                .clone()
                .await
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            for pipeline in &self.pipelines {
                pipeline.compile();
            }
            Ok(())
        }
    }
}

struct Plane {
    extent: [u32; 2],
    buffer: wgpu::Buffer,
}
impl Plane {
    fn new(device: &wgpu::Device, extent: [u32; 2]) -> Self {
        Self {
            extent,
            buffer: buffer(
                device,
                u64::from(extent[0]) * u64::from(extent[1]) * 16,
                "local tone plane",
            ),
        }
    }
}
fn buffer(device: &wgpu::Device, size: u64, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
#[derive(Default)]
struct Params {
    size: [u32; 4],
    aux: [u32; 4],
    values: [f32; 4],
    weights: [f32; 4],
}

/// Encoding is platform independent. Native workers wait between bounded
/// batches; browser hosts can await the same submissions without device.poll.
pub(crate) struct Builder {
    device: PipelineDevice,
    pipelines: Arc<Pipelines>,
    document: [u32; 2],
    space: RgbSpace,
    sums: Plane,
    original: Vec<Plane>,
    remapped: Vec<Plane>,
    horizontal: Vec<Plane>,
    detail: Vec<Plane>,
    dummy: wgpu::Buffer,
    texture: wgpu::TextureView,
}
impl Builder {
    /// All private image buffers, final output and a conservative allowance for
    /// range-reduction intermediates. Pipeline/driver allocations are separate.
    pub(crate) fn allocation_bound(document: [u32; 2]) -> Result<u64, String> {
        let extent = guide_extent(document)?;
        let area = |e: [u32; 2]| u64::from(e[0]) * u64::from(e[1]);
        let mut cells = 2 * area(extent) + 1; // sums and output/header
        let mut current = extent;
        loop {
            cells += 3 * area(current); // original, remapped, accumulated detail
            if current == [1, 1] {
                break;
            }
            cells += u64::from(current[0].div_ceil(2)) * u64::from(current[1]);
            current = current.map(|n| n.div_ceil(2));
        }
        Ok(cells * 16 + 64 * 1024)
    }
    pub(crate) async fn prepare_pipelines(device: &PipelineDevice) -> Result<(), String> {
        device
            .tone_pipelines
            .get_or_init(|| Arc::new(Pipelines::new(device)))
            .prepare()
            .await
    }
    pub(crate) fn new(
        device: &PipelineDevice,
        document: [u32; 2],
        space: RgbSpace,
    ) -> Result<Self, String> {
        let extent = guide_extent(document)?;
        if 16 + u64::from(extent[0]) * u64::from(extent[1]) * 16
            > u64::from(device.limits().max_storage_buffer_binding_size)
        {
            return Err("Local tone guide exceeds GPU buffer limit".into());
        }
        let pipelines = device
            .tone_pipelines
            .get_or_init(|| Arc::new(Pipelines::new(device)))
            .clone();
        let mut extents = vec![extent];
        while extents.last() != Some(&[1, 1]) {
            extents.push(extents.last().unwrap().map(|n| n.div_ceil(2)));
        }
        let planes = || extents.iter().map(|&e| Plane::new(device, e)).collect();
        let texture = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("local tone unused source"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Ok(Self {
            device: device.clone(),
            pipelines,
            document,
            space,
            sums: Plane::new(device, extent),
            original: planes(),
            remapped: planes(),
            detail: planes(),
            horizontal: extents[..extents.len() - 1]
                .iter()
                .map(|e| Plane::new(device, [e[0].div_ceil(2), e[1]]))
                .collect(),
            dummy: buffer(device, 16, "local tone unused input"),
            texture,
        })
    }
    fn encode(
        &self,
        encoder: &mut CommandEncoder,
        entry: usize,
        params: Params,
        inputs: [Option<&wgpu::Buffer>; 3],
        output: &wgpu::Buffer,
        texture: Option<&wgpu::TextureView>,
        groups: [u32; 2],
    ) {
        let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("local tone parameters"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: true,
        });
        {
            let mut bytes = uniform
                .slice(..)
                .get_mapped_range_mut()
                .expect("new mapped uniform");
            let mut packed = [0u8; 64];
            packed[..16].copy_from_slice(params.size.map(u32::to_ne_bytes).as_flattened());
            packed[16..32].copy_from_slice(params.aux.map(u32::to_ne_bytes).as_flattened());
            packed[32..48].copy_from_slice(params.values.map(f32::to_ne_bytes).as_flattened());
            packed[48..].copy_from_slice(params.weights.map(f32::to_ne_bytes).as_flattened());
            bytes.copy_from_slice(&packed);
        }
        uniform.unmap();
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(ENTRIES[entry]),
            layout: &self.pipelines.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: inputs[0].unwrap_or(&self.dummy).as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: inputs[1].unwrap_or(&self.dummy).as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: inputs[2].unwrap_or(&self.dummy).as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(texture.unwrap_or(&self.texture)),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(ENTRIES[entry]),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines.pipelines[entry]);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(groups[0], groups[1], 1);
    }
    pub(crate) fn reduce(
        &self,
        encoder: &mut CommandEncoder,
        texture: &wgpu::Texture,
        origin: [u32; 2],
    ) {
        let extent = self.sums.extent;
        let scale = [
            extent[0] as f32 / self.document[0] as f32,
            extent[1] as f32 / self.document[1] as f32,
        ];
        let start =
            std::array::from_fn::<_, 2, _>(|i| (origin[i] as f32 * scale[i]).floor() as u32);
        let end = [
            (origin[0] + texture.width()) as f32 * scale[0],
            (origin[1] + texture.height()) as f32 * scale[1],
        ]
        .map(|n| n.ceil() as u32);
        let weights = layer_core::color::hdr::sdr_luminance_weights(self.space);
        self.encode(
            encoder,
            0,
            Params {
                size: [extent[0], extent[1], start[0], start[1]],
                aux: [origin[0], origin[1], self.document[0], self.document[1]],
                weights: [weights[0], weights[1], weights[2], 0.],
                ..Default::default()
            },
            [None; 3],
            &self.sums.buffer,
            Some(&texture.create_view(&Default::default())),
            [
                (end[0] - start[0]).div_ceil(8),
                (end[1] - start[1]).div_ceil(8),
            ],
        );
    }
    pub(crate) fn statistics(&self, encoder: &mut CommandEncoder) -> wgpu::Buffer {
        let base = &self.original[0];
        self.encode(
            encoder,
            1,
            dims(base.extent),
            [Some(&self.sums.buffer), None, None],
            &base.buffer,
            None,
            groups(base.extent),
        );
        let mut count = base.extent[0] * base.extent[1];
        let mut source = base.buffer.clone();
        let mut partial = false;
        loop {
            let next = count.div_ceil(256);
            let target = buffer(&self.device, u64::from(next) * 16, "local tone range");
            self.encode(
                encoder,
                2,
                Params {
                    size: [count, 0, 0, 0],
                    aux: [u32::from(partial), 0, 0, 0],
                    ..Default::default()
                },
                [Some(&source), None, None],
                &target,
                None,
                [next, 1],
            );
            if next == 1 {
                return target;
            }
            count = next;
            source = target;
            partial = true;
        }
    }
    fn pyramid(&self, encoder: &mut CommandEncoder, planes: &[Plane]) {
        for (level, horizontal) in self.horizontal.iter().enumerate() {
            let source = &planes[level];
            let target = &planes[level + 1];
            let mut p = dims(horizontal.extent);
            p.aux = [source.extent[0], source.extent[1], 0, 0];
            self.encode(
                encoder,
                3,
                p,
                [Some(&source.buffer), None, None],
                &horizontal.buffer,
                None,
                groups(horizontal.extent),
            );
            let mut p = dims(target.extent);
            p.aux = [horizontal.extent[0], horizontal.extent[1], 1, 0];
            self.encode(
                encoder,
                3,
                p,
                [Some(&horizontal.buffer), None, None],
                &target.buffer,
                None,
                groups(target.extent),
            );
        }
    }
    pub(crate) fn prepare(&self, encoder: &mut CommandEncoder) {
        self.pyramid(encoder, &self.original);
        // The coarsest detail is zero, but must retain original coverage.
        let last = self.original.len() - 1;
        self.encode(
            encoder,
            8,
            dims(self.original[last].extent),
            [Some(&self.original[last].buffer), None, None],
            &self.detail[last].buffer,
            None,
            [1, 1],
        );
    }
    pub(crate) fn anchor(
        &self,
        encoder: &mut CommandEncoder,
        low: f32,
        step: f32,
        intervals: u32,
        index: u32,
    ) {
        let base = &self.original[0];
        let mut p = dims(base.extent);
        p.values[0] = low + index as f32 * step;
        self.encode(
            encoder,
            4,
            p,
            [Some(&base.buffer), None, None],
            &self.remapped[0].buffer,
            None,
            groups(base.extent),
        );
        self.pyramid(encoder, &self.remapped);
        for level in 0..self.original.len() - 1 {
            let original = &self.original[level];
            let coarse = &self.remapped[level + 1];
            let mut p = dims(original.extent);
            p.aux = [coarse.extent[0], coarse.extent[1], 0, 0];
            p.values = [low, step, intervals as f32, index as f32];
            self.encode(
                encoder,
                5,
                p,
                [
                    Some(&original.buffer),
                    Some(&self.remapped[level].buffer),
                    Some(&coarse.buffer),
                ],
                &self.detail[level].buffer,
                None,
                groups(original.extent),
            );
        }
    }
    pub(crate) fn finish(&self, encoder: &mut CommandEncoder, peak: f32) -> Arc<GpuToneGuide> {
        for level in (0..self.original.len() - 1).rev() {
            let original = &self.original[level];
            let coarse = &self.detail[level + 1];
            let mut p = dims(original.extent);
            p.aux = [coarse.extent[0], coarse.extent[1], 0, 0];
            self.encode(
                encoder,
                6,
                p,
                [Some(&original.buffer), None, Some(&coarse.buffer)],
                &self.detail[level].buffer,
                None,
                groups(original.extent),
            );
        }
        let extent = self.original[0].extent;
        let output = buffer(
            &self.device,
            16 + u64::from(extent[0]) * u64::from(extent[1]) * 16,
            "GPU local tone guide",
        );
        // Geometry is integer data. Bitcasting small dimensions through shader
        // floats permits denormal flushing on mobile/WebGPU implementations.
        use wgpu::util::DeviceExt;
        let dimensions = [extent[0], extent[1], self.document[0], self.document[1]];
        let bytes: Vec<u8> = dimensions.into_iter().flat_map(u32::to_le_bytes).collect();
        let header = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("local tone geometry"),
                contents: &bytes,
                usage: wgpu::BufferUsages::COPY_SRC,
            });
        encoder.copy_buffer_to_buffer(&header, 0, &output, 0, 16);
        let p = dims(extent);
        self.encode(
            encoder,
            7,
            p,
            [
                Some(&self.original[0].buffer),
                Some(&self.detail[0].buffer),
                None,
            ],
            &output,
            None,
            groups(extent),
        );
        Arc::new(GpuToneGuide {
            extent,
            document_extent: self.document,
            space: self.space,
            peak,
            device: (*self.device).clone(),
            buffer: output,
        })
    }
}
fn guide_extent(document: [u32; 2]) -> Result<[u32; 2], String> {
    if document.contains(&0) || document.iter().any(|&n| n > 32768) {
        return Err("Invalid local tone-map dimensions".into());
    }
    let scale = (document[0].max(document[1]) as f32 / LOCAL_GUIDE_EDGE as f32).max(1.);
    Ok(document.map(|n| (n as f32 / scale).ceil() as u32))
}
fn dims(e: [u32; 2]) -> Params {
    Params {
        size: [e[0], e[1], 0, 0],
        ..Default::default()
    }
}
fn groups(e: [u32; 2]) -> [u32; 2] {
    e.map(|n| n.div_ceil(8))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn wait(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    queue.on_submitted_work_done(move || {
        let _ = tx.send(Ok(()));
    });
    crate::raster::wait_mapping(device, &rx)
}
async fn read_buffer_async(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
) -> Result<Vec<u8>, String> {
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("local tone readback"),
        size: source.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(source, 0, &output, 0, source.size());
    queue.submit([encoder.finish()]);
    #[cfg(not(target_arch = "wasm32"))]
    let (tx, rx) = std::sync::mpsc::channel();
    #[cfg(target_arch = "wasm32")]
    let (tx, rx) = futures_channel::oneshot::channel();
    output.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r.map_err(|e| e.to_string()));
    });
    #[cfg(not(target_arch = "wasm32"))]
    crate::raster::wait_mapping(device, &rx)?;
    #[cfg(target_arch = "wasm32")]
    rx.await.map_err(|e| e.to_string())??;
    let bytes = output
        .slice(..)
        .get_mapped_range()
        .map_err(|e| e.to_string())?
        .to_vec();
    output.unmap();
    Ok(bytes)
}
pub(crate) async fn range_async(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
) -> Result<(f32, f32, u32, f32), String> {
    let bytes = read_buffer_async(device, queue, source).await?;
    let values: [f32; 4] =
        std::array::from_fn(|i| f32::from_ne_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
    if values[3] != 0. {
        return Err("Invalid HDR sample in local tone analysis".into());
    }
    let (low, high) = if values[0] > values[1] {
        (-24., -24.)
    } else {
        (values[0], values[1])
    };
    if !low.is_finite() || !high.is_finite() || !values[2].is_finite() {
        return Err("Non-finite local tone range".into());
    }
    let intervals = ((high - low) / 0.5).ceil().max(1.) as u32;
    Ok((
        low,
        ((high - low) / intervals as f32).max(0.00001),
        intervals,
        values[2],
    ))
}

pub(crate) async fn wait_async(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        wait(device, queue)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = device;
        let (tx, rx) = futures_channel::oneshot::channel();
        queue.on_submitted_work_done(move || {
            let _ = tx.send(());
        });
        rx.await.map_err(|e| e.to_string())
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
fn range(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
) -> Result<(f32, f32, u32, f32), String> {
    pollster::block_on(range_async(device, queue, source))
}
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use layer_core::color::{DocumentColor, SampleDepth, hdr::LocalToneBuilder};
    fn build(
        r: &crate::WgpuRasterizer,
        extent: [u32; 2],
        pixels: &[[f32; 4]],
        tile: u32,
    ) -> Result<Arc<GpuToneGuide>, String> {
        let builder = Builder::new(&r.device, extent, r.document_color.space)?;
        for y in (0..extent[1]).step_by(tile as usize) {
            for x in (0..extent[0]).step_by(tile as usize) {
                let size = [tile.min(extent[0] - x), tile.min(extent[1] - y)];
                let texture = r.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("tone oracle source"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba32Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let bytes: Vec<u8> = (y..y + size[1])
                    .flat_map(|row| {
                        pixels[(row * extent[0] + x) as usize
                            ..(row * extent[0] + x + size[0]) as usize]
                            .iter()
                            .flat_map(|p| p.iter().flat_map(|v| v.to_ne_bytes()))
                    })
                    .collect();
                r.queue.write_texture(
                    texture.as_image_copy(),
                    &bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size[0] * 16),
                        rows_per_image: None,
                    },
                    texture.size(),
                );
                let mut encoder = CommandEncoder::new(&r.device, &Default::default());
                builder.reduce(&mut encoder, &texture, [x, y]);
                encoder.submit(&r.queue);
            }
        }
        let mut encoder = CommandEncoder::new(&r.device, &Default::default());
        let stats = builder.statistics(&mut encoder);
        builder.prepare(&mut encoder);
        encoder.submit(&r.queue);
        let (low, step, intervals, peak) = range(&r.device, &r.queue, &stats)?;
        for index in 0..=intervals {
            let mut encoder = CommandEncoder::new(&r.device, &Default::default());
            builder.anchor(&mut encoder, low, step, intervals, index);
            encoder.submit(&r.queue);
        }
        let mut encoder = CommandEncoder::new(&r.device, &Default::default());
        let guide = builder.finish(&mut encoder, peak);
        encoder.submit(&r.queue);
        wait(&r.device, &r.queue)?;
        Ok(guide)
    }
    #[test]
    fn gpu_guide_matches_cpu_for_coverage_range_odd_sizes_and_tiled_reduction() {
        for space in RgbSpace::ALL {
            let r = crate::WgpuRasterizer::new_native_headless(DocumentColor {
                space,
                depth: SampleDepth::F32,
            })
            .unwrap();
            for (extent, pattern) in [
                ([1, 1], 0),
                ([1, 17], 1),
                ([31, 23], 2),
                ([1025, 769], 3),
                ([1537, 19], 4),
                ([7, 9], 5),
            ] {
                let mut pixels = Vec::new();
                let mut cpu = LocalToneBuilder::new(extent, space).unwrap();
                for y in 0..extent[1] {
                    let row: Vec<_> = (0..extent[0])
                        .map(|x| match pattern {
                            0 => [0.; 4],
                            1 => [0.125, 0.125, 0.125, 0.5],
                            5 => [-0.25, 0., 0., 0.5],
                            _ => {
                                let alpha = if (x / 13 + y / 17) % 7 == 0 {
                                    0.
                                } else if (x + y) % 11 == 0 {
                                    1e-8
                                } else {
                                    0.1 + 0.9 * ((x + y) % 11) as f32 / 10.
                                };
                                let luminance = 2f32.powf(-12. + 20. * x as f32 / extent[0] as f32)
                                    * if (x / 4 + y / 4) % 2 == 0 { 0.8 } else { 1.2 };
                                if alpha == 0. {
                                    return [1024., -999., 3., 0.];
                                }
                                [
                                    luminance * alpha,
                                    luminance * 0.6 * alpha,
                                    -0.025 * luminance * alpha,
                                    alpha,
                                ]
                            }
                        })
                        .collect();
                    cpu.push(&row).unwrap();
                    pixels.extend(row);
                }
                let expected = cpu.finish(|| false).unwrap();
                let gpu = build(&r, extent, &pixels, 127).unwrap();
                let actual = gpu.download(&r.queue).unwrap();
                assert_eq!(actual.extent, expected.extent);
                assert_eq!(actual.document_extent, extent);
                assert!((actual.peak - expected.peak).abs() <= 2e-6 * expected.peak);
                let mut errors = [0f32; 3];
                for (a, b) in actual.samples.iter().zip(&expected.samples) {
                    for c in 0..3 {
                        assert!(a[c].is_finite());
                        errors[c] = errors[c].max((a[c] - b[c]).abs());
                    }
                }
                eprintln!("GPU_TONE_ORACLE {space:?} {extent:?} maximum_error={errors:?}");
                assert!(
                    // Float32 source-area products may contract on the GPU;
                    // a guide coordinate near 768 has a 0.000061 ULP. Bound
                    // coverage separately from log-luminance/illumination.
                    errors[0] < 0.0003 && errors[1] < 0.0003 && errors[2] < 0.0001,
                    "{errors:?}"
                );
                // Source/working pixels are input-only, output handles remain immutable.
                let second = build(&r, [1, 1], &[[16., 16., 16., 1.]], 1).unwrap();
                assert_eq!(gpu.download(&r.queue).unwrap().samples, actual.samples);
                assert_eq!(second.document_extent, [1, 1]);
            }
            for pixel in [
                [f32::NAN, 0., 0., 1.],
                [1., 0., 0., -0.1],
                [1., 0., 0., 1.01],
            ] {
                assert!(build(&r, [1, 1], &[pixel], 1).is_err());
            }
        }
    }
}
