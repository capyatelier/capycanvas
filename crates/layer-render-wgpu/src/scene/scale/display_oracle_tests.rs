use super::*;
use layer_render::{Dab, DabBatch, DabBatchKind, ViewState};

const EXTENT: [u32; 2] = [2560, 1536];
const VIEW: [u32; 2] = [384, 256];
const FRAMES: usize = 20;
const STRICT: f32 = 2e-3;
const APPROXIMATE: f32 = 0.03;
const EDGE: f32 = 0.2;
const INK: f32 = 0.2;
const REPAIR_FRAMES: usize = 8;
const PUBLISH_FRAMES: usize = 64;
const FOUND: usize = 98304;
const EVICTED: usize = 8;

#[derive(Clone, Copy, Debug)]
enum Zoom { Actual, Half, Quarter, Fit }

impl Zoom {
    fn level(self) -> u32 { match self { Self::Actual => 0, Self::Half => 1, Self::Quarter => 2, Self::Fit => 3 } }
    fn view(self, pan: [u32; 2]) -> ViewState {
        let side = 1u32 << self.level();
        let size = match self { Self::Fit => EXTENT.map(|extent| extent / side), _ => VIEW };
        let origin = match self { Self::Actual => [1000, 600], Self::Half => [800, 400], Self::Quarter => [512, 256], Self::Fit => [0, 0] };
        let [x, y] = [0, 1].map(|axis| (origin[axis] + pan[axis] * side).min(EXTENT[axis] - size[axis] * side));
        let scale = 1. / side as f32;
        ViewState { width_px: size[0], height_px: size[1], document_to_surface: [scale, 0., 0., scale, -(x as f32) * scale, -(y as f32) * scale] }
    }
}

#[derive(Clone, Copy, Debug)]
struct Brush { preset: DefaultBrushPreset, radius: f32, color: [f32; 4], flow: f32 }

const G_PEN: Brush = Brush { preset: DefaultBrushPreset::GPen, radius: 36., color: [0.02, 0.02, 0.04, 1.], flow: 1. };
const WIDE_G_PEN: Brush = Brush { preset: DefaultBrushPreset::GPen, radius: 1100., color: [0.6, 0.05, 0.1, 1.], flow: 1. };
const PENCIL: Brush = Brush { preset: DefaultBrushPreset::Pencil, radius: 24., color: [0.1, 0.2, 0.7, 1.], flow: 0.9 };
const ERASER: Brush = Brush { preset: DefaultBrushPreset::Eraser, radius: 60., color: [1.; 4], flow: 1. };
const WIDE_PASTEL: Brush = Brush { preset: DefaultBrushPreset::PastelBlock, radius: 1000., color: [0.9, 0.7, 0.1, 1.], flow: 0.8 };

#[derive(Clone, Copy, Debug)]
struct Case { seed: u64, brush: Brush, zoom: Zoom, pan: bool, surround: bool, layered: bool, budget: Option<u64> }

macro_rules! cases {
    ($($name:ident: $case:expr,)*) => {$(
        #[test]
        fn $name() { crate::test_support::with_pace([2 * object_cache::PASS_TAPS, crate::frame_pace::IDLE_TAPS], || run(stringify!($name), $case)); }
    )*};
}

cases! {
    g_pen_fit: Case { seed: 1, brush: G_PEN, zoom: Zoom::Fit, pan: false, surround: false, layered: false, budget: None },
    g_pen_quarter_pan: Case { seed: 2, brush: G_PEN, zoom: Zoom::Quarter, pan: true, surround: false, layered: false, budget: None },
    wide_g_pen_half_pan: Case { seed: 3, brush: WIDE_G_PEN, zoom: Zoom::Half, pan: true, surround: false, layered: false, budget: None },
    wide_g_pen_fit_layered: Case { seed: 4, brush: WIDE_G_PEN, zoom: Zoom::Fit, pan: false, surround: false, layered: true, budget: None },
    pencil_actual_layered: Case { seed: 5, brush: PENCIL, zoom: Zoom::Actual, pan: true, surround: false, layered: true, budget: None },
    eraser_quarter_layered: Case { seed: 6, brush: ERASER, zoom: Zoom::Quarter, pan: false, surround: false, layered: true, budget: None },
    wide_pastel_fit: Case { seed: 7, brush: WIDE_PASTEL, zoom: Zoom::Fit, pan: false, surround: false, layered: false, budget: None },
    g_pen_fit_from_surround: Case { seed: 9, brush: G_PEN, zoom: Zoom::Fit, pan: false, surround: true, layered: false, budget: None },
}

struct Frame { dabs: Vec<Dab>, batches: Vec<DabBatch>, pan: [u32; 2] }

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn unit(&mut self) -> f32 { (self.next() >> 40) as f32 / (1u64 << 24) as f32 }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

fn document(layered: bool) -> (layer_core::Document, SourceTarget) {
    let mut doc = document_at(EXTENT);
    let target = primary_target(&doc);
    let backdrop = paint_occurrence(&mut doc, "Backdrop", Some(rgba8_source(EXTENT, |_, _| [210, 200, 180, 255])));
    insert_occurrence(&mut doc, backdrop, 1);
    paint_mut(&mut doc, 0).base = Some(layer_core::authored::PaintBase::new(rgba8_source(EXTENT, |x, y| {
        let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77); h ^= h >> 15; h = h.wrapping_mul(0x2C1B_3C6D); h ^= h >> 12;
        let noise = (h & 15) as u8;
        [40 + noise + (x * 120 / EXTENT[0]) as u8, 70 + noise + (y * 110 / EXTENT[1]) as u8, 150 - noise, 255]
    }).into()));
    if layered {
        let mut program = (*crate::tests::fixture("exposure").program()).clone();
        program.resolution = layer_core::EffectResolution::Display;
        let mut effect = layer_core::EffectInstance::new(Arc::new(program));
        effect.set("exposure", layer_core::EffectValue::Number(0.6)).unwrap();
        let handle = effect_occurrence(&mut doc, effect, "Oracle filter");
        coverage_mask(&mut doc, handle, [0, 0], Some(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 300., y: 200. }, layer_core::Point { x: 2100., y: 380. },
            layer_core::Point { x: 1900., y: 1400. }, layer_core::Point { x: 500., y: 1200. },
        ]).unwrap()));
        doc.artwork.occurrences.get_mut(handle).unwrap().opacity = 0.7;
        insert_occurrence(&mut doc, handle, 0);
    }
    (doc, target)
}

fn strokes(case: Case, target: SourceTarget, style: &layer_render::DabStyle) -> Vec<Frame> {
    let mut rng = Rng(case.seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
    let visible = display_mips::view_bounds(case.zoom.view([0, 0]), EXTENT, 0).unwrap();
    let centre = [(visible.min_x() + visible.max_x()) as f32 * 0.5, (visible.min_y() + visible.max_y()) as f32 * 0.5];
    let reach = [visible.width() as f32, visible.height() as f32].map(|side| (side * 0.45).min(case.brush.radius * 6.));
    let phase = rng.unit() * std::f32::consts::TAU;
    let path = |t: f32| [(centre[0] + reach[0] * (t + phase).sin()).clamp(0., EXTENT[0] as f32), (centre[1] + reach[1] * 0.8 * (2. * t).sin()).clamp(0., EXTENT[1] as f32)];
    let at = |t: f32| if t < 0. { [case.brush.radius * (t * 0.5 - 2.), path(0.)[1]] } else { path(t) };
    let step = 2.6 * std::f32::consts::PI / (FRAMES as f32 * 1.5);
    let dab = |point: [f32; 2], rng: &mut Rng| {
        let mut dab = crate::tests::test_dab(point, case.brush.color, case.brush.flow);
        dab.radii = [case.brush.radius * (0.85 + 0.15 * rng.unit()); 2];
        dab
    };
    let (mut t, mut stroke, mut started, mut pan) = (if case.surround { -1. } else { 0. }, 1, false, [0, 0]);
    let interruption = FRAMES / 3 + rng.below(3) as usize;
    (0..FRAMES).map(|index| {
        let ends = index + 1 == FRAMES || index == interruption;
        if case.pan && index == FRAMES / 2 { pan = [16 + rng.below(16) as u32, 8 + rng.below(16) as u32]; }
        let shape = if ends { 1 } else if index > 0 && rng.below(10) == 0 { 3 } else { rng.below(4) };
        let mut persistent = Vec::new();
        if shape == 1 || shape == 2 { for _ in 0..1 + rng.below(4) { t += step; persistent.push(dab(at(t), &mut rng)); } }
        let mut preview = Vec::new();
        if shape == 0 || shape == 2 {
            let drift = [rng.unit() - 0.5, rng.unit() - 0.5].map(|value| value * case.brush.radius);
            let mut s = t;
            for _ in 0..1 + rng.below(4) { s += step; let [x, y] = at(s); preview.push(dab([x + drift[0], y + drift[1]], &mut rng)); }
        }
        let damage = |dabs: &[Dab]| dabs.iter().map(|dab| dab.bounds()).reduce(|a, b| a.union(b)).unwrap();
        let batch = |kind, start, first: usize, dabs: &[Dab]| DabBatch { kind, stroke_id: layer_core::StrokeId(stroke), stroke_start: start, stroke_end: false,
            first_dab: first as u32, dab_count: dabs.len() as u32, ..crate::test_support::dab_batch(target, style.clone(), damage(dabs)) };
        let mut batches = Vec::new();
        if !persistent.is_empty() { batches.push(DabBatch { stroke_end: ends, ..batch(DabBatchKind::Persistent, !started, 0, &persistent) }); started = true; }
        if !preview.is_empty() { batches.push(batch(DabBatchKind::Preview, !started, persistent.len(), &preview)); }
        if ends && started { stroke += 1; started = false; t += 1.7; }
        Frame { dabs: persistent.into_iter().chain(preview).collect(), batches, pan }
    }).collect()
}

const SHADERS: &str = "
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var reduced: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var<uniform> window: vec4<u32>;
@compute @workgroup_size(8, 8) fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(reduced)) { return; }
    let start = window.xy + id.xy * window.z;
    let end = min(start + vec2(window.z), textureDimensions(source));
    var sum = vec4<f32>(0.);
    for (var y = start.y; y < end.y; y++) { for (var x = start.x; x < end.x; x++) { sum += textureLoad(source, vec2(x, y), 0); } }
    let count = f32(max(end.x, start.x) - start.x) * f32(max(end.y, start.y) - start.y);
    textureStore(reduced, id.xy, select(vec4<f32>(0.), sum / count, count > 0.));
}
struct Entry { index: u32, error: f32, pad: vec2<u32>, shown: vec4<f32>, expected: vec4<f32> }
struct Found { count: atomic<u32>, a: u32, b: u32, c: u32, entries: array<Entry, FOUND> }
@group(0) @binding(0) var shown: texture_2d<f32>;
@group(0) @binding(1) var expected: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> found: Found;
@compute @workgroup_size(8, 8) fn compare(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(shown);
    if any(id.xy >= size) { return; }
    let a = textureLoad(shown, id.xy, 0);
    let b = textureLoad(expected, id.xy, 0);
    let d = abs(a - b);
    let error = max(max(d.x, d.y), max(d.z, d.w));
    if !(error <= STRICT) {
        let slot = atomicAdd(&found.count, 1u);
        if slot < FOUNDu { found.entries[slot] = Entry(id.y * size.x + id.x, error, vec2(0u), a, b); }
    }
}
";

fn pipeline(device: &wgpu::Device, entry: &str) -> wgpu::ComputePipeline {
    let source = SHADERS.replace("STRICT", &format!("{STRICT:?}")).replace("FOUND", &FOUND.to_string());
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("display oracle"), source: wgpu::ShaderSource::Wgsl(source.into()) });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(entry), layout: None, module: &module, entry_point: Some(entry), compilation_options: Default::default(), cache: None })
}

fn dispatch(device: &wgpu::Device, queue: &wgpu::Queue, pipeline: &wgpu::ComputePipeline, resources: &[wgpu::BindingResource<'_>], size: [u32; 2]) {
    let entries = resources.iter().enumerate().map(|(binding, resource)| wgpu::BindGroupEntry { binding: binding as u32, resource: resource.clone() }).collect::<Vec<_>>();
    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &pipeline.get_bind_group_layout(0), entries: &entries });
    let mut commands = device.create_command_encoder(&Default::default());
    {
        let mut pass = commands.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline); pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
    }
    queue.submit([commands.finish()]);
}

fn texture(device: &wgpu::Device, size: [u32; 2], usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor { label: Some("display oracle"), size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba32Float, usage, view_formats: &[] })
}

struct Found { index: usize, error: f32, shown: [f32; 4], expected: [f32; 4] }

struct Oracle { subject: WgpuRasterizer, reference: WgpuRasterizer, presenter: ViewportPresenter, reduce: wgpu::ComputePipeline, compare: wgpu::ComputePipeline }

impl Oracle {
    fn new(doc: &layer_core::Document, budget: Option<u64>) -> Self {
        let renderer = |budget: Option<u64>| {
            let mut r = match budget {
                Some(bytes) => crate::memory::memory_tests::renderer(doc.composition().color, layer_core::DeviceMemoryFacts { budget_bytes: Some(bytes), ..Default::default() }).unwrap(),
                None => WgpuRasterizer::new_native_headless(doc.composition().color).unwrap(),
            };
            layer_render::CanvasRenderer::set_working_layer(&mut r, doc.working.occurrence);
            r
        };
        let subject = renderer(budget);
        let mut reference = renderer(None);
        reference.test.reference = true;
        let presenter = crate::test_support::float_presenter(&subject);
        let reduce = pipeline(&reference.device, "reduce");
        let compare = pipeline(&subject.device, "compare");
        Self { subject, reference, presenter, reduce, compare }
    }

    fn publish_reference(&mut self, frame: FramePacket<'_>) -> Result<(), String> {
        let previews: Vec<_> = frame.dab_batches.iter().filter(|batch| batch.kind == DabBatchKind::Preview).cloned().collect();
        let idle = FramePacket { dabs: if previews.is_empty() { &[] } else { frame.dabs }, dab_batches: &previews, ..frame };
        crate::test_support::submit_ready(&mut self.reference, frame).unwrap();
        for _ in 0..PUBLISH_FRAMES {
            if !publication_pending(&self.reference) { return Ok(()); }
            self.reference.wait_idle().unwrap();
            crate::test_support::submit_ready(&mut self.reference, idle).unwrap();
        }
        Err(format!("the reference did not publish within {PUBLISH_FRAMES} frames"))
    }

    fn reduced(&self, origin: [u32; 2], size: [u32; 2], side: u32) -> Vec<u8> {
        let reduced = texture(&self.reference.device, size, wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let window = wgpu::util::DeviceExt::create_buffer_init(&*self.reference.device, &wgpu::util::BufferInitDescriptor { label: Some("display oracle window"),
            contents: &[origin[0], origin[1], side, 0].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(), usage: wgpu::BufferUsages::UNIFORM });
        let source = crate::test_support::document_texture(&self.reference).create_view(&Default::default());
        dispatch(&self.reference.device, &self.reference.queue, &self.reduce, &[wgpu::BindingResource::TextureView(&source),
            wgpu::BindingResource::TextureView(&reduced.create_view(&Default::default())), window.as_entire_binding()], size);
        crate::layer_tests::page_bytes(&self.reference, &reduced)
    }

    fn mismatches(&mut self, view: ViewState, level: u32) -> (Vec<Found>, Vec<[f32; 4]>) {
        let side = 1u32 << level;
        let size = [view.width_px, view.height_px];
        let bytes = self.reduced(texel_origin(view, side).map(|texel| texel * side), size, side);
        let expected = texture(&self.subject.device, size, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        self.subject.queue.write_texture(expected.as_image_copy(), &bytes, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * 16), rows_per_image: None }, expected.size());
        let (shown, target) = create_target(&self.subject.device, size, wgpu::TextureFormat::Rgba32Float, "display oracle presented frame");
        self.presenter.present(&self.subject, &target, view, [0.; 4]).unwrap();
        let found = self.subject.device.create_buffer(&wgpu::BufferDescriptor { label: Some("display oracle mismatches"), size: 16 + 48 * FOUND as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        dispatch(&self.subject.device, &self.subject.queue, &self.compare, &[wgpu::BindingResource::TextureView(&shown.create_view(&Default::default())),
            wgpu::BindingResource::TextureView(&expected.create_view(&Default::default())), found.as_entire_binding()], size);
        let bytes = crate::test_support::buffer_bytes(&self.subject, &found);
        let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let count = (word(0) as usize).min(FOUND);
        let found = (0..count).map(|slot| {
            let entry = 16 + slot * 48;
            let vector = |offset: usize| std::array::from_fn(|channel| f32::from_bits(word(entry + offset + channel * 4)));
            Found { index: word(entry) as usize, error: f32::from_bits(word(entry + 4)), shown: vector(16), expected: vector(32) }
        }).collect();
        (found, texels(&crate::layer_tests::page_bytes(&self.subject, &shown)))
    }
}

fn texel_origin(view: ViewState, side: u32) -> [u32; 2] {
    [view.document_to_surface[4], view.document_to_surface[5]].map(|offset| (-offset * side as f32).round() as u32 / side)
}

fn texels(bytes: &[u8]) -> Vec<[f32; 4]> {
    bytes.as_chunks::<16>().0.iter().map(|texel| std::array::from_fn(|channel| f32::from_le_bytes(texel[channel * 4..channel * 4 + 4].try_into().unwrap()))).collect()
}

fn difference(a: [f32; 4], b: [f32; 4]) -> f32 { (0..4).map(|channel| (a[channel] - b[channel]).abs()).fold(0., f32::max) }

struct Inked { origin: [u32; 2], width: u32, before: Vec<[f32; 4]>, most: Vec<f32> }

impl Inked {
    fn new(oracle: &Oracle, views: impl Iterator<Item = ViewState>, level: u32) -> Self {
        let side = 1u32 << level;
        let [min, max] = views.map(|view| {
            let origin = texel_origin(view, side);
            [origin, [origin[0] + view.width_px, origin[1] + view.height_px]]
        }).reduce(|[a, b], [c, d]| [[a[0].min(c[0]), a[1].min(c[1])], [b[0].max(d[0]), b[1].max(d[1])]]).unwrap();
        let size = [max[0] - min[0], max[1] - min[1]];
        let before = texels(&oracle.reduced(min.map(|texel| texel * side), size, side));
        Self { origin: min, width: size[0], most: vec![0.; before.len()], before }
    }
    fn index(&self, texel: [u32; 2]) -> usize { ((texel[1] - self.origin[1]) * self.width + texel[0] - self.origin[0]) as usize }
    fn reverts(&self, texel: [u32; 2], found: &Found) -> bool {
        let index = self.index(texel);
        self.most[index] > INK && difference(found.shown, self.before[index]) <= APPROXIMATE && difference(found.expected, self.before[index]) > INK
    }
    fn present(&mut self, origin: [u32; 2], width: u32, shown: &[[f32; 4]]) {
        for (surface, value) in shown.iter().enumerate() {
            let index = self.index([origin[0] + surface as u32 % width, origin[1] + surface as u32 / width]);
            self.most[index] = self.most[index].max(difference(*value, self.before[index]));
        }
    }
}

fn near_edge<'a>(mut dabs: impl Iterator<Item = &'a Dab>, texel: [u32; 2], level: u32, band: f32) -> bool {
    let side = (1u32 << level) as f32;
    let [x0, y0] = texel.map(|value| value as f32 * side);
    dabs.any(|dab| {
        let [cx, cy] = [dab.center.x, dab.center.y];
        let near = (cx.clamp(x0, x0 + side) - cx).hypot(cy.clamp(y0, y0 + side) - cy);
        let far = (cx - x0).abs().max((cx - x0 - side).abs()).hypot((cy - y0).abs().max((cy - y0 - side).abs()));
        near <= dab.radii[0] + band && far >= dab.radii[0] - band
    })
}

fn batch_dabs(frame: &Frame, kind: DabBatchKind) -> impl Iterator<Item = &Dab> {
    frame.batches.iter().filter(move |batch| batch.kind == kind).flat_map(|batch| &frame.dabs[batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize])
}

fn retained_keys(r: &WgpuRasterizer) -> std::collections::HashSet<pages::Key> {
    r.scale_display.as_ref().unwrap().pages.lock().unwrap().retained_pages().into_iter().map(|page| page.key).collect()
}

fn packet_for<'a>(base: FramePacket<'a>, zoom: Zoom, index: usize, frame: &'a Frame) -> FramePacket<'a> {
    FramePacket { dabs: &frame.dabs, dab_batches: &frame.batches, view: zoom.view(frame.pan), time_seconds: (index + 1) as f32 / 120., ..base }
}

fn run(name: &str, case: Case) {
    let (doc, target) = document(case.layered);
    let style = layer_render::DabStyle { blend_space: doc.composition().blend, ..crate::layer_tests::preset_style(case.brush.preset) };
    let sequence = strokes(case, target, &style);
    let outside = |dab: &Dab| dab.bounds().max.x < 0.;
    assert!(!case.surround || sequence.iter().any(|frame| batch_dabs(frame, DabBatchKind::Preview).next().is_some() && batch_dabs(frame, DabBatchKind::Preview).all(outside)),
        "{name}: the stream predicts a stroke wholly in the surround");
    let level = case.zoom.level();
    let base = FramePacket { composite_all: false, blend_space: doc.composition().blend, view: case.zoom.view([0, 0]), ..packet(doc.scene(), EXTENT) };
    let mut oracle = Oracle::new(&doc, case.budget);
    let subject = &mut oracle.subject;
    crate::test_support::submit_settled(subject, FramePacket { composite_all: true, ..base });
    for (index, frame) in sequence.iter().enumerate() { crate::test_support::submit_ready(subject, packet_for(base, case.zoom, index, frame)).unwrap(); }
    crate::test_support::submit_settled(subject, FramePacket { reset_layers: true, composite_all: true, ..base });
    while crate::compile::tests::queued_recipes(&subject.compilation) != 0 { subject.compilation.check().unwrap(); subject.wait_idle().unwrap(); }
    crate::test_support::submit_settled(subject, FramePacket { composite_all: true, ..base });
    crate::test_support::submit_settled(&mut oracle.reference, FramePacket { composite_all: true, ..base });
    assert_eq!(oracle.subject.scale_display.as_ref().unwrap().plan.level, level, "{name}: the view shows level {level}");
    let mut inked = Inked::new(&oracle, sequence.iter().map(|frame| case.zoom.view(frame.pan)), level);
    let mut owing = BTreeMap::<[u32; 2], usize>::new();
    let mut painted = Vec::new();
    let mut seen = retained_keys(&oracle.subject);
    let settle = Frame { dabs: Vec::new(), batches: Vec::new(), pan: sequence.last().unwrap().pan };
    for (index, frame) in sequence.iter().chain(std::iter::once(&settle)).enumerate() {
        let settled = index == sequence.len();
        let frame_packet = packet_for(base, case.zoom, index, frame);
        painted.extend(batch_dabs(frame, DabBatchKind::Persistent).copied());
        if settled { crate::test_support::submit_settled(&mut oracle.subject, frame_packet); } else { crate::test_support::submit_ready(&mut oracle.subject, frame_packet).unwrap(); }
        if let Err(error) = oracle.publish_reference(FramePacket { composite_all: true, ..frame_packet }) { panic!("{name} seed {} frame {index}: {error}", case.seed); }
        if !settled { seen.extend(retained_keys(&oracle.subject)); }
        let cache = oracle.subject.scale_display.as_ref().unwrap();
        let labels = cache.shown_cells().into_iter().collect::<BTreeMap<_, _>>();
        let mislabelled = cache.mislabelled_cells();
        assert!(mislabelled.is_empty(), "{name} seed {} frame {index} level {level}: cells {mislabelled:?} show pixels labelled with inputs that are no longer current and are not queued", case.seed);
        let preview_level = oracle.subject.preview_level;
        let view = frame_packet.view;
        let side = 1u32 << level;
        let origin = texel_origin(view, side);
        let mut owed = BTreeSet::new();
        let (mismatches, shown) = oracle.mismatches(view, level);
        for found in mismatches {
            let surface = [found.index as u32 % view.width_px, found.index as u32 / view.width_px];
            let texel = [origin[0] + surface[0], origin[1] + surface[1]];
            if texel[0] * side >= EXTENT[0] || texel[1] * side >= EXTENT[1] { continue; }
            let cell = texel.map(|value| value / PAGE_SIZE);
            let label = if settled { Some(true) } else { labels.get(&cell).copied().flatten() };
            let edge = preview_level > 0 && near_edge(batch_dabs(frame, DabBatchKind::Preview), texel, level, (1u32 << preview_level) as f32)
                || label == Some(false) && near_edge(painted.iter().chain(batch_dabs(frame, DabBatchKind::Preview)), texel, level, (1u32 << level.max(preview_level)) as f32);
            let tolerance = if edge { EDGE } else if label == Some(true) { STRICT } else { APPROXIMATE };
            if found.error <= tolerance { continue; }
            if label.is_none() {
                assert!(!inked.reverts(texel, &found), "{name} seed {} frame {index} level {level}: surface texel {surface:?} (display texel {texel:?}, pending cell {cell:?}) shows the picture before the stroke {:?} after an earlier frame showed ink there, where the reference shows {:?}",
                    case.seed, found.shown, found.expected);
                owed.insert(cell); continue;
            }
            panic!("{name} seed {} frame {index} level {level}: surface texel {surface:?} (display texel {texel:?}, cell {cell:?}, {}) shows {:?} where the reference shows {:?}: error {} beyond {tolerance}",
                case.seed, if edge { "dab edge" } else if label == Some(true) { "exact cell" } else { "reduced cell" }, found.shown, found.expected, found.error);
        }
        inked.present(origin, view.width_px, &shown);
        owing.retain(|cell, _| owed.contains(cell));
        for cell in owed { *owing.entry(cell).or_default() += 1; }
        if let Some((cell, frames)) = owing.iter().find(|(_, frames)| **frames > REPAIR_FRAMES) {
            panic!("{name} seed {} frame {index} level {level}: cell {cell:?} has shown pixels that differ from the reference while pending for {frames} frames", case.seed);
        }
    }
    let retained = retained_keys(&oracle.subject);
    let evicted = seen.iter().filter(|key| !retained.contains(key)).count();
    assert!(case.budget.is_none() || evicted > EVICTED, "{name}: the tight budget evicts more than {EVICTED} pages, not {evicted}");
}

#[test]
fn a_prediction_that_touches_no_shown_cell_publishes_at_once_and_leaves_the_renderer_idle() {
    let (doc, target) = document(false);
    let style = layer_render::DabStyle { blend_space: doc.composition().blend, ..crate::layer_tests::preset_style(DefaultBrushPreset::GPen) };
    let scene = Some((doc.scene().owner(), doc.scene().revision()));
    let between = ViewState { width_px: VIEW[0], height_px: VIEW[1], document_to_surface: [0.55, 0., 0., 0.55, 0., 0.] };
    for (view, x) in [(Zoom::Fit.view([0, 0]), -90.), (Zoom::Actual.view([0, 0]), -90.), (Zoom::Actual.view([0, 0]), 100.), (between, -90.)] {
        let scale = view.document_to_surface[0];
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        layer_render::CanvasRenderer::set_working_layer(&mut r, doc.working.occurrence);
        let base = FramePacket { composite_all: false, blend_space: doc.composition().blend, view, ..packet(doc.scene(), EXTENT) };
        crate::test_support::submit_settled(&mut r, FramePacket { composite_all: true, ..base });
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.content_revision() == scene && (view != between || cache.pixels.committed_next().is_some()),
            "at scale {scale} the settled view presents the scene{}", if view == between { " through its half level" } else { "" });
        let mut dab = crate::tests::test_dab([x, 700.], G_PEN.color, 1.);
        dab.radii = [36.; 2];
        let batch = DabBatch { kind: DabBatchKind::Preview, stroke_end: false, ..crate::test_support::dab_batch(target, style.clone(), dab.bounds()) };
        let frame = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), time_seconds: 0.01, ..base };
        for held in 0..3 {
            crate::test_support::submit_ready(&mut r, frame).unwrap();
            assert!(!publication_pending(&r) && r.scale_display.as_ref().unwrap().content_revision() == scene,
                "at scale {scale}: a prediction at x {x} that touches no shown cell publishes and presents the scene in held frame {held}");
            r.wait_idle().unwrap();
        }
        let mut withdrawn = 0;
        while withdrawn < PUBLISH_FRAMES && r.next_frame() == layer_render::FrameDemand::Work { crate::test_support::submit_ready(&mut r, base).unwrap(); r.wait_idle().unwrap(); withdrawn += 1; }
        assert!(withdrawn < PUBLISH_FRAMES, "at scale {scale}: the renderer still asks for work {withdrawn} frames after the prediction at x {x} is withdrawn: {:?}", r.outstanding_terms());
    }
}

#[test]
fn a_root_a_blocked_prediction_left_unstamped_publishes_while_the_stroke_continues_off_view() {
    let (doc, target) = document(false);
    let style = layer_render::DabStyle { blend_space: doc.composition().blend, ..crate::layer_tests::preset_style(DefaultBrushPreset::GPen) };
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r, doc.working.occurrence);
    let base = FramePacket { composite_all: false, blend_space: doc.composition().blend, view: Zoom::Actual.view([0, 0]), ..packet(doc.scene(), EXTENT) };
    crate::test_support::submit_settled(&mut r, FramePacket { composite_all: true, ..base });
    let predicted = page_rect([4, 2]);
    {
        let cache = r.scale_display.as_ref().unwrap();
        evict_composite_pages(cache, &[predicted]);
        cache.invalidate_cells(cache.reached_cells(DocRect::from(predicted)), None);
        let display = r.scene_tiers.display.clone();
        cache.refuse(None, predicted, &display.blocked(crate::residency::Class::Visible, display.limit() + 1).expect("a unit larger than the display grant is refused"));
    }
    let stroke = |r: &mut WgpuRasterizer, x: f32, kind: DabBatchKind, step: usize| {
        let mut dab = crate::tests::test_dab([x, 640.], G_PEN.color, 1.);
        dab.radii = [36.; 2];
        let batch = DabBatch { kind, stroke_start: step == 0, stroke_end: false, ..crate::test_support::dab_batch(target, style.clone(), dab.bounds()) };
        crate::test_support::submit_ready(r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), time_seconds: 0.01 * (step + 1) as f32, ..base }).unwrap();
        r.wait_idle().unwrap();
    };
    stroke(&mut r, 1150., DabBatchKind::Preview, 0);
    let cache = r.scale_display.as_ref().unwrap();
    assert!(cache.contact_id().is_none() && !cache.displays(predicted) && cache.pixels.committed_root().is_some_and(|root| root.authored().is_none()),
        "the predicted cell keeps its old record, which leaves the root unstamped");
    for step in 1..4 {
        stroke(&mut r, 100. + 8. * step as f32, DabBatchKind::Persistent, step);
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.contact_id().is_none() && !publication_pending(&r), "frame {step} of the stroke continuing off view publishes once the prediction is withdrawn");
    }
}
