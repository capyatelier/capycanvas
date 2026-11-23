use super::*;
use layer_core::{ArtworkStatisticsRequest, color::histogram::{Histogram,Waveform}};
use wgpu::util::DeviceExt;

const WORDS: u64 = 1043;
const SHARDS: u64 = 64;

pub(crate) struct StatisticsPipeline {
    layout: wgpu::BindGroupLayout,
    count: Deferred<wgpu::ComputePipeline>,
    fold: Deferred<wgpu::ComputePipeline>,
    resolve: Deferred<wgpu::ComputePipeline>,
}
impl StatisticsPipeline {
    fn new(device: &PipelineDevice) -> Arc<Self> {
        device.statistics_pipeline.get_or_init(|| {
            let storage = |binding, read_only| crate::bindings::buffer(binding, wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Storage { read_only }, false, None);
            let layout = crate::bindings::layout(device, "artwork statistics", &[
                crate::bindings::texture(0, wgpu::ShaderStages::COMPUTE, false),
                crate::bindings::buffer(1, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Uniform, false, None),
                storage(2, true), storage(3, false), storage(4, false), storage(5, true),
            ]);
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("artwork statistics"), source: wgpu::ShaderSource::Wgsl(concat!(include_str!("../float_number.wgsl"),"\n",include_str!("statistics.wgsl")).into()),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("artwork statistics"), bind_group_layouts: &[Some(&layout)], immediate_size: 0,
            });
            let pipeline = |entry| Deferred::compute_module(device, "artwork statistics", &pipeline_layout, &shader, entry);
            Arc::new(Self { layout, count: pipeline("count"), fold: pipeline("fold"), resolve:pipeline("resolve") })
        }).clone()
    }
}

impl SnapshotGpu {
    pub async fn artwork_statistics(&self, request: ArtworkStatisticsRequest, control: CaptureControl) -> Result<Histogram, String> {
        control.check().map_err(|e| e.to_string())?;
        let (mut snapshot, output) = self.artwork_capture(&request.query, control.clone()).await?;
        let selection = request.selection.then(|| request.query.selection.clone().map(Arc::new)).flatten();
        if request.selection && selection.is_none() { return Err("Select an area to inspect".into()); }
        let extent = snapshot.extent;
        let mut histogram = Histogram::new(snapshot.color());
        if let layer_core::ArtworkSource::EffectInput(id) | layer_core::ArtworkSource::EffectChannels(id) = request.query.source
            && let Some(effect)=request.query.snapshot.view().effect(id)
                && matches!(effect.program.id.as_ref(),"curves"|"levels") {
                histogram.domain = match (effect.choice("domain"),effect.value("hdr_stops")) {
                    (Some("log_hdr"),Some(layer_core::EffectValue::Number(stops)))=>layer_core::color::histogram::HistogramDomain::CurveLog {stops:*stops},
                    _=>layer_core::color::histogram::HistogramDomain::Encoded,
                };
            }
        let pipeline = StatisticsPipeline::new(&self.device);
        self.device.compilation().map_err(|e|e.to_string())?.prepare_all([&pipeline.count, &pipeline.fold, &pipeline.resolve]).await?;
        let buffer = |label, size| self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label), size, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let waveform_words = if request.waveform {Waveform::WORDS as u64} else {0};
        let shards = buffer("histogram shards", WORDS * SHARDS * 4+4*(1+256*256)+waveform_words*8*4);
        let summary = buffer("histogram counts", (2*WORDS+waveform_words)*4);
        let weights = histogram.color.space.to_xyz()[1];
        let coefficients = [weights[0], 1. - weights[0] - weights[2], weights[2]];
        let boundaries: Vec<_> = [0, 3].into_iter().flat_map(|channel| histogram.boundaries(channel)).chain(coefficients)
            .flat_map(|v| {
                let bits = v.to_bits();
                [bits as u32, ((bits >> 32) as u32 & 0xfffff) | 0x100000, ((bits >> 52) as i32 - 1023) as u32, (v.log2() as f32).to_bits()]
            }).flat_map(u32::to_le_bytes).collect();
        let boundaries = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("histogram boundaries"), contents: &boundaries, usage: wgpu::BufferUsages::STORAGE,
        });
        let no_selection = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("histogram unrestricted"), contents: &[0; 48], usage: wgpu::BufferUsages::STORAGE,
        });
        let reserved = shards.size()+summary.size()*2+boundaries.size()+no_selection.size()+48;
        let mut folded=None;
        snapshot.capture_query_gpu(output,request.preview,16,reserved,selection.as_ref(),|r,view,region,encoder| {
            let weights = histogram.color.space.to_xyz()[1];
            let words = [region.min_x(), region.min_y(), region.width(), region.height(), extent[0], extent[1],
                u32::from(request.preview), u32::from(histogram.color.depth.is_float() && histogram.domain==layer_core::color::histogram::HistogramDomain::Artwork),
                (weights[0] as f32).to_bits(), (weights[2] as f32).to_bits(), u32::from(request.waveform), 0];
            let area = r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("histogram region"), contents: &words.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let group = crate::bindings::group(&r.device, "histogram chunk", &pipeline.layout, [
                wgpu::BindingResource::TextureView(view), area.as_entire_binding(), boundaries.as_entire_binding(),
                shards.as_entire_binding(), summary.as_entire_binding(),
                selection.as_ref().and(r.selection_clip.buffer.as_ref()).unwrap_or(&no_selection).as_entire_binding(),
            ]);
            encoder.clear_buffer(&shards,WORDS*SHARDS*4,Some(4));
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline.count);pass.set_bind_group(0, &group, &[]);
            let points = if request.preview {
                let begin = |p:u32,extent:u32| ((2*u64::from(p)*u64::from(extent.min(256))+u64::from(extent)-1)/(2*u64::from(extent))) as u32;
                (begin(region.max_x(),extent[0])-begin(region.min_x(),extent[0]))*(begin(region.max_y(),extent[1])-begin(region.min_y(),extent[1]))
            } else {region.width()*region.height()};
            pass.dispatch_workgroups(points.div_ceil(64).min(64), 1, 1);
            pass.set_pipeline(&pipeline.resolve);
            pass.dispatch_workgroups(points.div_ceil(64).min(64),1,1);
            drop(pass);
            folded=Some(group);Ok(())
        }).await.map_err(|e|e.to_string())?;
        if let Some(group)=folded {
            let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
            let mut pass=encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline.fold);pass.set_bind_group(0,&group,&[]);
            pass.dispatch_workgroups(((WORDS+waveform_words) as u32).div_ceil(64),1,1);drop(pass);encoder.submit(&self.queue);
        }
        let bytes = crate::local_tone::read_buffer_async(&self.device, &self.queue, &summary).await?;
        control.check().map_err(|e| e.to_string())?;
        let counts: Vec<_> = bytes[..2*WORDS as usize*4].as_chunks::<4>().0.iter().map(|b| u64::from(u32::from_le_bytes(*b))).collect();
        let (low, carries) = counts.split_at(WORDS as usize);
        let words: Vec<_> = low.iter().zip(carries).map(|(low, carries)| low | carries << 32).collect();
        if words[1042] != 0 { return Err("The artwork contains invalid color values".into()); }
        for (i, channel) in histogram.channels.iter_mut().enumerate() {
            channel.bins.copy_from_slice(&words[i*256..(i+1)*256]);
            [channel.below, channel.above, channel.black, channel.white] = words[1024+i*4..1028+i*4].try_into().unwrap();
        }
        histogram.pixels = words[1040];histogram.transparent = words[1041];
        if request.waveform {
            histogram.waveform=Some(Waveform {counts:bytes[2*WORDS as usize*4..].as_chunks::<4>().0.iter()
                .map(|b|u32::from_le_bytes(*b)).collect()});
        }
        Ok(histogram)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn storage(device: &PipelineDevice, contents: &[u8]) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST })
    }
    pub(crate) fn words(device: &PipelineDevice, queue: &wgpu::Queue, buffer: &wgpu::Buffer) -> Vec<u32> {
        pollster::block_on(crate::local_tone::read_buffer_async(device, queue, buffer)).unwrap().as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect()
    }
    pub(crate) fn pixels(device: &PipelineDevice, queue: &wgpu::Queue) -> wgpu::TextureView {
        let texture = device.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        queue.write_texture(texture.as_image_copy(), &[0.5f32, 0.5, 0.5, 1.].repeat(64).into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8 * 16), rows_per_image: None }, texture.size());
        texture.create_view(&Default::default())
    }
    pub(crate) fn dispatch(device: &PipelineDevice, queue: &wgpu::Queue, pipeline: &wgpu::ComputePipeline, group: &wgpu::BindGroup, workgroups: u32) {
        let mut encoder = submission::CommandEncoder::new(device, &Default::default());
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline); pass.set_bind_group(0, group, &[]); pass.dispatch_workgroups(workgroups, 1, 1); drop(pass);
        encoder.submit(queue);
    }

    #[test]
    fn histogram_counts_carry_past_four_billion_pixels() {
        let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let (device, queue) = (&r.device, &r.queue);
        let pipeline = StatisticsPipeline::new(device);
        pollster::block_on(device.compilation().unwrap().prepare_all([&pipeline.fold])).unwrap();
        let shards = storage(device, &(1u32 << 31).to_le_bytes().repeat((WORDS * SHARDS) as usize));
        let summary = storage(device, &vec![0; 2 * WORDS as usize * 4]);
        let zeros = storage(device, &[0; 48]);
        let area = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &[0; 48], usage: wgpu::BufferUsages::UNIFORM });
        let view = pixels(device, queue);
        let group = crate::bindings::group(device, "histogram carry", &pipeline.layout, [wgpu::BindingResource::TextureView(&view),
            area.as_entire_binding(), zeros.as_entire_binding(), shards.as_entire_binding(), summary.as_entire_binding(), zeros.as_entire_binding()]);
        dispatch(device, queue, &pipeline.fold, &group, (WORDS as u32).div_ceil(64));
        let counts = words(device, queue, &summary);
        assert!(counts[..WORDS as usize].iter().all(|low| *low == 0));
        assert!(counts[WORDS as usize..].iter().all(|carries| *carries == 32), "64 shards of 2^31 pixels hold 2^37");
    }

}
