use super::*;

pub(super) const SLOTS: usize = 16;
pub(super) const RECORD_BYTES: u64 = 256;
const BASE: u32 = 1;
const NATIVE: u32 = 2;
const OVERLAY: u32 = 4;
const CONTRIBUTION: u32 = 8;
const MASK: u32 = 16;
const INVERTED: u32 = 32;
const END: u32 = 64;

pub(super) struct Layer {
    pub base: Option<wgpu::TextureView>,
    pub overlay: Option<(wgpu::TextureView, bool)>,
    pub mask: Option<(wgpu::TextureView, bool)>,
    pub opacity: f32,
    pub coverage: f32,
}
impl Layer {
    fn slots(&self) -> usize {
        usize::from(self.base.is_some()) + usize::from(self.overlay.is_some()) + usize::from(self.mask.is_some())
    }
}

#[derive(Clone)]
pub(super) struct Fold {
    pub target: wgpu::TextureView,
    pub transfer: wgpu::Buffer,
    cleared: Option<wgpu::TextureView>,
    constant: [f32; 4],
    backdrop: bool,
    slots: Vec<(wgpu::TextureView, u32)>,
    opacity: [f32; SLOTS],
    coverage: [f32; SLOTS],
    convert: Convert,
    pub output_convert: Convert,
    origin: [i32; 2],
    clip: [i32; 4],
    pub layers: u64,
}
impl Fold {
    fn new(target: wgpu::TextureView, transfer: wgpu::Buffer, convert: Convert) -> Self {
        Self { target, transfer, cleared: None, constant: [0.; 4], backdrop: false, slots: Vec::with_capacity(SLOTS), opacity: [1.; SLOTS], coverage: [1.; SLOTS],
            convert, output_convert: Convert::None, origin: [0; 2], clip: [0, 0, PAGE_SIZE as i32, PAGE_SIZE as i32], layers: 0 }
    }
    pub fn constant(target: wgpu::TextureView, transfer: wgpu::Buffer, convert: Convert, color: wgpu::Color) -> Self {
        Self { cleared: Some(target.clone()), constant: [color.r, color.g, color.b, color.a].map(|value| value as f32), ..Self::new(target, transfer, convert) }
    }
    pub fn absorbs(&self, cleared: &wgpu::TextureView) -> bool { self.cleared.as_ref() == Some(cleared) }
    pub fn over(target: wgpu::TextureView, transfer: wgpu::Buffer, convert: Convert, backdrop: wgpu::TextureView) -> Self {
        let mut fold = Self::new(target, transfer, convert);
        fold.backdrop = true;
        fold.slots.push((backdrop, 0));
        fold
    }
    pub fn extends(&self, target: &wgpu::TextureView, layer: &Layer, convert: Convert) -> bool {
        self.target == *target && self.convert == convert && self.output_convert == Convert::None && self.origin == [0; 2]
            && self.slots.len() + layer.slots() <= SLOTS
    }
    pub fn push(&mut self, layer: Layer) {
        let native = |view: &wgpu::TextureView| if view.texture().format() == wgpu::TextureFormat::Rgba8Unorm { NATIVE } else { 0 };
        let first = self.slots.len();
        if let Some(base) = layer.base { let role = BASE | native(&base); self.slots.push((base, role)); }
        if let Some((overlay, contribution)) = layer.overlay {
            let role = OVERLAY | if contribution { CONTRIBUTION } else { 0 } | native(&overlay);
            self.slots.push((overlay, role));
        }
        if let Some((mask, inverted)) = layer.mask { self.slots.push((mask, MASK | if inverted { INVERTED } else { 0 })); }
        self.slots[first].1 |= BASE;
        let last = self.slots.len() - 1;
        self.slots[last].1 |= END;
        self.coverage[first] = layer.coverage;
        self.opacity[last] = layer.opacity;
        self.layers += 1;
    }
    pub fn reads(&self, view: &wgpu::TextureView) -> bool { self.slots.iter().any(|(slot, _)| slot == view) }
    pub fn retarget(&mut self, from: &wgpu::TextureView, destination: &wgpu::TextureView, origin: [i64; 2], clip: PixelRect) -> bool {
        if self.target != *from || self.origin != [0; 2] || self.reads(from) || self.reads(destination) { return false; }
        self.target = destination.clone();
        self.origin = origin.map(|value| value as i32);
        self.clip = [clip.min_x(), clip.min_y(), clip.max_x(), clip.max_y()].map(|value| value as i32);
        true
    }
    pub fn views(&self, empty: &wgpu::TextureView) -> [wgpu::TextureView; SLOTS] {
        std::array::from_fn(|index| self.slots.get(index).map_or_else(|| empty.clone(), |(view, _)| view.clone()))
    }
    pub fn record(&self) -> [u8; RECORD_BYTES as usize] {
        let mut words = [0u32; RECORD_BYTES as usize / 4];
        words[..4].copy_from_slice(&[self.slots.len() as u32, u32::from(self.backdrop), self.convert.code() as u32, self.output_convert.code() as u32]);
        words[4..8].copy_from_slice(&self.constant.map(f32::to_bits));
        words[8..12].copy_from_slice(&[self.origin[0] as u32, self.origin[1] as u32, 255f32.to_bits(), 0]);
        words[12..16].copy_from_slice(&self.clip.map(|value| value as u32));
        for (index, (_, role)) in self.slots.iter().enumerate() { words[16 + index] = *role; }
        words[32..48].copy_from_slice(&self.opacity.map(f32::to_bits));
        words[48..64].copy_from_slice(&self.coverage.map(f32::to_bits));
        let mut bytes = [0u8; RECORD_BYTES as usize];
        for (output, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) { *output = word.to_ne_bytes(); }
        bytes
    }
}

fn source() -> String {
    let bindings: String = (0..SLOTS).map(|slot| format!("@group(1) @binding({slot}) var slot{slot}: texture_2d<f32>;\n")).collect();
    let cases: String = (0..SLOTS).map(|slot| if slot + 1 < SLOTS { format!("            case {slot}u: {{ value = textureLoad(slot{slot}, p, 0); }}\n") }
        else { format!("            default: {{ value = textureLoad(slot{slot}, p, 0); }}\n") }).collect();
    include_str!("../scene_fold.wgsl").replace("FOLD_SLOT_BINDINGS", &bindings).replace("FOLD_TRANSFER", &SLOTS.to_string()).replace("FOLD_SLOT_CASES", &cases)
}

#[derive(Clone)]
pub(crate) struct Pipelines {
    pub records: wgpu::BindGroupLayout,
    inputs: wgpu::BindGroupLayout,
    pub pipeline: Deferred<wgpu::ComputePipeline>,
}
impl Pipelines {
    pub fn new(device: &PipelineDevice, output: &wgpu::BindGroupLayout) -> Self {
        let records = crate::bindings::layout(device, "scene fold records", &[crate::bindings::buffer(
            0, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Uniform, true, NonZeroU64::new(RECORD_BYTES))]);
        let entries: Vec<_> = (0..SLOTS as u32).map(|binding| crate::bindings::texture(binding, wgpu::ShaderStages::COMPUTE, false))
            .chain([crate::bindings::buffer(SLOTS as u32, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Storage { read_only: true }, false,
                wgpu::BufferSize::new(crate::native_tiles::transfer::TABLE_BYTES))]).collect();
        let inputs = crate::bindings::layout(device, "scene fold inputs", &entries);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene fold"), bind_group_layouts: &[Some(&records), Some(&inputs), Some(output)], immediate_size: 0,
        });
        let shader = Deferred::wgsl(device, "scene fold", compose_wgsl(&[&working_color::source(device.working_space()),
            include_str!("../scene_convert.wgsl"), &source()]));
        let pipeline = Deferred::compute(device, "scene fold", &layout, &shader, "fold_layers");
        Self { records, inputs, pipeline }
    }
    pub fn records(&self, device: &wgpu::Device, buffer: &wgpu::Buffer) -> wgpu::BindGroup {
        crate::bindings::group(device, "scene fold records", &self.records,
            [wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: 0, size: NonZeroU64::new(RECORD_BYTES) })])
    }
    pub fn inputs(&self, device: &wgpu::Device, views: &[wgpu::TextureView; SLOTS], transfer: &wgpu::Buffer) -> wgpu::BindGroup {
        crate::bindings::group(device, "scene fold inputs", &self.inputs,
            views.iter().map(wgpu::BindingResource::TextureView).chain([transfer.as_entire_binding()]))
    }
}
