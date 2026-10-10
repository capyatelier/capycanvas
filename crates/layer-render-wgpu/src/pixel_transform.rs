//! Affine, perspective and mesh cut-and-place over a bounded set of immutable
//! source views. Manual Float32 interpolation applies selection and
//! premultiplied color together.
use crate::submission::ColorPass;
use super::{Deferred, PipelineDevice, Uploads};
use layer_core::{ImageTransform, Interpolation, Projective};

pub(super) const TRANSFORM_SLOTS: usize = 16;
/// Taps per axis a minified pixel averages in drag previews.
pub(super) const PREVIEW_TAPS: u32 = 4;
/// Taps per axis a minified pixel averages at most in exact passes: commits
/// and capture.
pub(super) const EXACT_TAPS: u32 = 16;
/// Mesh source positions per attachment pixel, bound in the last source slot;
/// uncovered pixels hold UNCOVERED.
pub(super) const POSITIONS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;
pub(super) const UNCOVERED: f64 = -3.0e38;
/// Destination-to-source rows, attachment origin and options of one job, how
/// a display level composites it, and the source bounds and views it reads.
const RECORD_BYTES: u64 = 112 + (1 + TRANSFORM_SLOTS as u64) * 16;
/// A display level flag: see `DisplayLevel::encode`.
const ENCODED: f32 = 512.;
const BINDING_CAPACITY: usize = 4096;

pub(super) struct TransformTile<'a> {
    pub view: &'a wgpu::TextureView,
    pub origin: [i32; 2],
    pub extent: [u32; 2],
}

pub struct TransformSource {
    binding: wgpu::BindGroup,
}

pub(super) struct TiledTransformRecord<'a> {
    /// The layer pixel at the attachment's origin.
    pub origin: [i64; 2],
    pub sources: &'a [[u32; 2]],
    pub source_size: [u32; 2],
    /// x, y, width, height of the display level texels a job draws.
    pub texels: [u32; 4],
    pub unmoved: bool,
    pub clear: bool,
}
/// Draw a display level: each texel is the mean of `side` x `side` layer
/// pixels within `extent`, times `opacity`, over the premultiplied
/// `backdrop`. `side` divides 16. With `encode`, the display holds a
/// Perceptual composite: the mean is encoded before it lies over the
/// backdrop, which is encoded already.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DisplayLevel {
    pub side: u32,
    pub opacity: f32,
    pub extent: [u32; 2],
    pub backdrop: [f32; 4],
    pub encode: bool,
}
/// One region drawn into a shared attachment by `encode_batch`.
pub(super) struct BatchDraw<'a> {
    pub source: &'a TransformSource,
    pub job: usize,
    /// x, y, width, height in the attachment.
    pub scissor: [u32; 4],
}

pub(super) struct PlacementDraw {
    pub records: wgpu::BindGroup,
    pub source: TransformSource,
    pub target: wgpu::BindGroup,
    pub offset: u32,
    pub size: [u32; 2],
    pipeline: Deferred<wgpu::ComputePipeline>,
}

/// Which of a layer's unmoved pixels an identity transform draws into a
/// display level.
#[derive(Clone, Copy)]
pub(super) enum Part {
    Whole = 0,
    /// Only the pixels the selection moves.
    Selected = 32,
    /// Only the pixels the selection keeps in place.
    Kept = 64,
}

/// A cached source binding's selection, source views and mesh positions.
type SourceKey = (wgpu::Buffer, Vec<wgpu::TextureView>, Option<wgpu::TextureView>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Mapping { Copy, Affine, Projective, Mesh }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key { mapping: Mapping, filter: u8, area: bool }
impl Key {
    fn new(transform: &ImageTransform, taps: u32) -> Self {
        if transform.is_identity() { return Self { mapping: Mapping::Copy, filter: 0, area: false }; }
        Self {
            mapping: if transform.placement.mesh.is_some() { Mapping::Mesh }
                else if transform.as_affine().is_some() { Mapping::Affine } else { Mapping::Projective },
            filter: transform.placement.interpolation as u8,
            area: transform.placement.interpolation != Interpolation::Nearest && taps > 1,
        }
    }
}

pub(super) struct Kernels {
    device: PipelineDevice,
    scalar: bool,
    visibility: bool,
    render: std::sync::Mutex<std::collections::HashMap<Key, Deferred<wgpu::RenderPipeline>>>,
    compute: std::sync::Mutex<std::collections::HashMap<(Key, bool), Deferred<wgpu::ComputePipeline>>>,
    render_layout: wgpu::PipelineLayout,
    compute_layout: wgpu::PipelineLayout,
    display_layout: wgpu::BindGroupLayout,
    layout: wgpu::BindGroupLayout,
    source_layout: wgpu::BindGroupLayout,
    empty_selection: wgpu::Buffer,
    stride: u32,
}
impl Kernels {
    fn render(&self, key: Key) -> Deferred<wgpu::RenderPipeline> {
        self.render.lock().unwrap().entry(key).or_insert_with(|| {
            let shader = shader(&self.device, key, self.scalar, self.visibility, None);
            let (device, layout) = (self.device.for_recipe(), self.render_layout.clone());
            let format = if self.scalar { device.scalar_format() } else { device.working_format() };
            Deferred::pipeline(move |mode| super::fullscreen_pipeline_targets_with_constants_recipe(
                mode, &device, &layout, &shader, "fragment_main",
                &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                &[], "transform pixels"))
        }).clone()
    }
    fn compute(&self, key: Key, display: bool) -> Deferred<wgpu::ComputePipeline> {
        self.compute.lock().unwrap().entry((key, display)).or_insert_with(|| {
            let shader = shader(&self.device, key, self.scalar, self.visibility, Some(display));
            Deferred::compute(&self.device, if display { "transform display level" } else { "placed pixels" },
                &self.compute_layout, &shader, if display { "display_main" } else { "placement_main" })
        }).clone()
    }
}

pub struct PixelTransform {
    pub(super) kernels: std::sync::Arc<Kernels>,
    display_target: crate::bindings::CachedBinding<wgpu::TextureView>,
    bindings: std::collections::HashMap<SourceKey, (u64, wgpu::BindGroup)>,
    binding_frame: u64,
    uniforms: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    capacity: u64,
    records: Vec<u8>,
    next_record: u64,
    key: Key,
}
impl PixelTransform {
    pub(super) fn staged(device: &PipelineDevice, scalar: bool) -> Self {
        Self::create(device, scalar, false)
    }
    pub(super) fn passes(device: &PipelineDevice) -> [Self; 3] {
        [(false, false), (true, false), (true, true)]
            .map(|(scalar, visibility)| Self::create(device, scalar, visibility))
    }
    fn create(
        device: &PipelineDevice,
        scalar: bool,
        visibility: bool,
    ) -> Self {
        let stages = wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
        let layout = crate::bindings::layout(device, "transform records", &[crate::bindings::buffer(
            0,
            stages,
            wgpu::BufferBindingType::Uniform,
            true,
            wgpu::BufferSize::new(RECORD_BYTES),
        )]);
        let mut entries: Vec<_> = (0..TRANSFORM_SLOTS as u32)
            .map(|binding| crate::bindings::texture(binding, stages, false))
            .collect();
        entries.push(crate::bindings::buffer(
            TRANSFORM_SLOTS as u32,
            stages,
            wgpu::BufferBindingType::Storage { read_only: true },
            false,
            wgpu::BufferSize::new(48),
        ));
        let source_layout = crate::bindings::layout(device, "transform sources and selection", &entries);
        let display_layout = crate::bindings::layout(device, "transform display level", &[
            crate::bindings::storage_texture(
                u32::from(scalar),
                wgpu::ShaderStages::COMPUTE,
                if scalar { wgpu::TextureFormat::R32Float } else { wgpu::TextureFormat::Rgba32Float },
                wgpu::StorageTextureAccess::WriteOnly,
            ),
        ]);
        let pipeline_layout = |layouts: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("transform pixels"),
                bind_group_layouts: layouts,
                immediate_size: 0,
            })
        };
        let render_layout = pipeline_layout(&[Some(&layout), Some(&source_layout)]);
        let compute_layout = pipeline_layout(&[Some(&layout), Some(&source_layout), Some(&display_layout)]);
        Self::from_kernels(std::sync::Arc::new(Kernels {
            device: device.for_recipe(), scalar, visibility, render: Default::default(), compute: Default::default(),
            render_layout, compute_layout, display_layout, layout, source_layout,
            empty_selection: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("transform selects entire layer"), size: 48,
                usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false,
            }),
            stride: (RECORD_BYTES as u32).next_multiple_of(device.limits().min_uniform_buffer_offset_alignment),
        }))
    }
    pub(super) fn from_kernels(kernels: std::sync::Arc<Kernels>) -> Self {
        Self { kernels, display_target: Default::default(), bindings: Default::default(),
            binding_frame: 0, uniforms: None, capacity: 0, records: Vec::new(), next_record: 0, key: Key::new(&Default::default(), 1) }
    }
    pub(super) fn fork(&self) -> Self {
        Self::from_kernels(self.kernels.clone())
    }
    pub(super) fn render_pipeline(&self, transform: &ImageTransform, taps: u32) -> Deferred<wgpu::RenderPipeline> {
        self.kernels.render(Key::new(transform, taps))
    }
    pub(super) fn placement_pipeline(&self, transform: &ImageTransform, taps: u32) -> Deferred<wgpu::ComputePipeline> {
        self.kernels.compute(Key::new(transform, taps), false)
    }
    pub(super) fn display_pipeline(&self) -> Deferred<wgpu::ComputePipeline> {
        self.kernels.compute(Key::new(&Default::default(), 1), true)
    }

    /// Inputs are consumed before another source-cache neighborhood is prepared.
    /// `bounds` and view origins are in document coordinates; missing texels use
    /// the declared background, including sparse gaps inside those bounds.
    pub(super) fn source_views(
        &mut self,
        device: &wgpu::Device,
        tiles: &[TransformTile<'_>],
        bounds: [i32; 4],
        selection: Option<&wgpu::Buffer>,
        positions: Option<&wgpu::TextureView>,
        fallback: &wgpu::TextureView,
    ) -> Result<TransformSource, &'static str> {
        let slots = TRANSFORM_SLOTS - usize::from(positions.is_some());
        if tiles.len() > slots
            || bounds[2] <= 0
            || bounds[3] <= 0
            || !valid_extent([bounds[0], bounds[1]], [bounds[2] as u32, bounds[3] as u32])
            || tiles.iter().any(|t| !valid_extent(t.origin, t.extent))
        {
            return Err("Invalid transform source views");
        }
        if selection.is_some_and(|b| {
            b.size() < 48
                || b.size() > device.limits().max_storage_buffer_binding_size
                || !b.usage().contains(wgpu::BufferUsages::STORAGE)
        }) {
            return Err("Invalid transform selection");
        }
        let selection = selection.unwrap_or(&self.kernels.empty_selection);
        let key = (
            selection.clone(),
            tiles.iter().map(|t| t.view.clone()).collect::<Vec<_>>(),
            positions.cloned(),
        );
        let binding = if let Some((used, binding)) = self.bindings.get_mut(&key) {
            *used = self.binding_frame;
            binding.clone()
        } else {
            let views = (0..TRANSFORM_SLOTS).map(|i| match tiles.get(i) {
                Some(tile) => tile.view,
                None if i + 1 == TRANSFORM_SLOTS => positions.unwrap_or(fallback),
                None => fallback,
            });
            let binding = crate::bindings::group(
                device,
                "transform sources and selection",
                &self.kernels.source_layout,
                views.map(wgpu::BindingResource::TextureView).chain([selection.as_entire_binding()]),
            );
            if self.bindings.len() >= BINDING_CAPACITY {
                for recent in [self.binding_frame.saturating_sub(1), self.binding_frame] {
                    self.bindings.retain(|_, (used, _)| *used >= recent);
                    if self.bindings.len() < BINDING_CAPACITY {
                        break;
                    }
                }
            }
            self.bindings.insert(key, (self.binding_frame, binding.clone()));
            binding
        };
        Ok(TransformSource { binding })
    }
    /// Begin a submitted frame. Multiple encodes in that frame use distinct
    /// record ranges. Submit the previous frame before calling this again.
    pub fn begin_frame(&mut self) {
        self.next_record = 0;
        self.binding_frame += 1;
    }
    #[allow(clippy::mutable_key_type)]
    pub fn retain_source_bindings(
        &mut self,
        views: &[&wgpu::TextureView],
        selection: Option<&wgpu::Buffer>,
    ) {
        let allowed: std::collections::HashSet<_> = views.iter().copied().collect();
        self.bindings.retain(|(buffer, bound, _), _| {
            (*buffer == self.kernels.empty_selection || selection.is_some_and(|s| s == buffer))
                && bound.iter().all(|v| allowed.contains(v))
        });
    }
    /// Size the records for `jobs` drawn in one frame ahead of that frame.
    pub(super) fn reserve(&mut self, device: &wgpu::Device, jobs: u64) {
        let end = jobs * u64::from(self.kernels.stride);
        if end > self.capacity {
            self.capacity = end
                .next_power_of_two()
                .min(device.limits().max_buffer_size)
                .min(u64::from(u32::MAX))
                & !3;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("transform records"),
                size: self.capacity,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let binding = crate::bindings::group(device, "transform records", &self.kernels.layout, [
                wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &buffer, offset: 0, size: wgpu::BufferSize::new(RECORD_BYTES), }),
            ]);
            self.uniforms = Some((buffer, binding));
        }
    }
    /// Upload one record per job and return the offset of the first. A part
    /// other than the whole layer draws only into a display level. A minified
    /// pixel averages at most `taps` taps per axis.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_tiled(
        &mut self,
        device: &wgpu::Device,
        uploads: &mut Uploads,
        encoder: &mut crate::submission::CommandEncoder,
        bounds: [i32; 4],
        background: f32,
        transform: &ImageTransform,
        taps: u32,
        jobs: &[TiledTransformRecord<'_>],
        display: Option<(DisplayLevel, Part)>,
    ) -> Result<u32, &'static str> {
        let rows = inverse_rows(transform)?;
        self.key = Key::new(transform, taps);
        let identity = transform.is_identity();
        let stride = u64::from(self.kernels.stride);
        let bytes = jobs.len() as u64 * stride;
        let end = self.next_record + bytes;
        if end > device.limits().max_buffer_size.min(u64::from(u32::MAX)) {
            return Err("Too many transform regions");
        }
        let offset = self.next_record as u32;
        if jobs.is_empty() {
            return Ok(offset);
        }
        self.reserve(device, end / stride);
        self.records.resize(bytes as usize, 0);
        let part = display.map_or(Part::Whole, |(_, part)| part);
        for (job, record) in jobs.iter().zip(self.records.chunks_exact_mut(stride as usize)) {
            let values = region_record(
                rows,
                taps,
                job.origin.map(|v| v as f32),
                2. * f32::from(job.unmoved || identity)
                    + f32::from(part as u8)
                    + 256. * f32::from(transform.keep_source)
                    + 1024. * f32::from(job.clear),
                background,
                display.map(|(level, _)| level),
                job.texels.map(|v| v as f32),
            );
            let metadata = source_metadata(
                bounds,
                job.sources.iter().map(|c| (c.map(|v| (v * super::PAGE_SIZE) as i32), job.source_size)),
            );
            let bytes = values.into_iter().map(f32::to_le_bytes).chain(metadata.into_iter().map(i32::to_le_bytes));
            for (dst, value) in record.as_chunks_mut::<4>().0.iter_mut().zip(bytes) {
                *dst = value;
            }
        }
        uploads
            .write_at(
                encoder,
                &self.uniforms.as_ref().unwrap().0,
                self.next_record,
                &self.records,
            )
            .map_err(|_| "Could not upload transform records")?;
        self.next_record = end;
        Ok(offset)
    }
    /// Draw prepared regions of several jobs into one attachment with a
    /// single pass, clearing the attachment first or keeping earlier passes.
    pub(super) fn encode_batch(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        attachment: &wgpu::TextureView,
        clear: bool,
        offset: u32,
        draws: &[BatchDraw<'_>],
    ) {
        let pipeline = self.kernels.render(self.key);
        let mut pass = encoder.color_pass(
            "batched transform regions",
            attachment,
            if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load },
        );
        pass.set_pipeline(&pipeline);
        for draw in draws {
            pass.set_bind_group(0, &self.uniforms.as_ref().unwrap().1, &[offset + draw.job as u32 * self.kernels.stride]);
            pass.set_bind_group(1, &draw.source.binding, &[]);
            let [x, y, w, h] = draw.scissor;
            pass.set_scissor_rect(x, y, w, h);
            pass.draw(0..3, 0..1);
        }
    }
    pub(super) fn placement_draw(&self, device: &wgpu::Device, target: &wgpu::TextureView,
        source: TransformSource, offset: u32) -> PlacementDraw {
        let output = self.display_target.get(target.clone(), || device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("placed pixels destination"), layout: &self.kernels.display_layout,
            entries: &[wgpu::BindGroupEntry { binding: u32::from(self.kernels.scalar), resource: wgpu::BindingResource::TextureView(target) }],
        }));
        PlacementDraw { records: self.uniforms.as_ref().unwrap().1.clone(), source, target: output.clone(), offset,
            size: [target.texture().width(), target.texture().height()], pipeline: self.kernels.compute(self.key, false) }
    }

    pub(super) fn encode_placement<'a>(&'a self, pass: &mut wgpu::ComputePass<'a>, draw: &'a PlacementDraw) {
        pass.set_pipeline(&draw.pipeline);
        pass.set_bind_group(0, &draw.records, &[draw.offset]);
        pass.set_bind_group(1, &draw.source.binding, &[]);
        pass.set_bind_group(2, &draw.target, &[]);
        pass.dispatch_workgroups(draw.size[0].div_ceil(8), draw.size[1].div_ceil(8), 1);
    }
    /// Draw prepared jobs straight into a display level, `side` layer pixels
    /// per texel, keeping its other texels. Each draw's scissor holds the
    /// job's texels.
    pub(super) fn encode_display(
        &self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        level: &wgpu::TextureView,
        side: u32,
        offset: u32,
        draws: &[BatchDraw<'_>],
    ) {
        let target = self.display_target.get(level.clone(), || {
            crate::bindings::group(device, "transform display level", &self.kernels.display_layout, [
                wgpu::BindingResource::TextureView(level),
            ])
        });
        let pipeline = self.kernels.compute(self.key, true);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("transform into display level"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(2, &target, &[]);
        let texels = 16 / side;
        for draw in draws {
            pass.set_bind_group(0, &self.uniforms.as_ref().unwrap().1, &[offset + draw.job as u32 * self.kernels.stride]);
            pass.set_bind_group(1, &draw.source.binding, &[]);
            let [_, _, w, h] = draw.scissor;
            pass.dispatch_workgroups(w.div_ceil(texels), h.div_ceil(texels), 1);
        }
    }
    /// Retained scratch only; source/targets are owned by the transaction host.
    pub fn storage_bytes(&self) -> u64 {
        self.capacity + 48
    }
}
/// The taps per axis an exact pass averages over a minified pixel: enough
/// for the largest source step of an affine map, and the most allowed for
/// a perspective or mesh, whose steps vary.
pub(super) fn exact_taps(transform: &ImageTransform) -> u32 {
    let Some(affine) = transform.as_affine().and_then(|a| a.inverse()) else {
        return EXACT_TAPS;
    };
    let [a, b, c, d, _, _] = affine.0;
    let step = a.hypot(b).max(c.hypot(d));
    ((step + 0.501).floor() as u32).clamp(1, EXACT_TAPS)
}

/// Rows mapping a destination pixel to homogeneous source coordinates. A mesh
/// reads its source positions from a texture instead.
pub(super) fn inverse_rows(transform: &ImageTransform) -> Result<[[f32; 3]; 3], &'static str> {
    let invalid = "Transform must be finite and invertible";
    let projective = if transform.placement.mesh.is_some() {
        return Ok(match transform.source_from_owner.unwrap_or(Projective::IDENTITY).0 {
            [a,b,c,d,e,f,g,h,i] => [[a,b,c],[d,e,f],[g,h,i]],
        });
    } else { transform.projective().ok_or(invalid)? };
    if let Some(affine) = projective.as_affine() {
        let [a, b, c, d, x, y] = affine.inverse().ok_or(invalid)?.0;
        return Ok([[a, c, x], [b, d, y], [0., 0., 1.]]);
    }
    let m = projective.inverse().ok_or(invalid)?.0;
    Ok([[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]])
}

/// Uniform values of one region drawn into an attachment whose origin is
/// `target` in layer pixels.
fn region_record(
    [x, y, w]: [[f32; 3]; 3],
    taps: u32,
    target: [f32; 2],
    flags: f32,
    background: f32,
    display: Option<DisplayLevel>,
    texels: [f32; 4],
) -> [f32; 28] {
    let level = display.unwrap_or(DisplayLevel {
        side: 1,
        opacity: 1.,
        extent: [0; 2],
        backdrop: [0.; 4],
        encode: false,
    });
    let [r, g, b, a] = level.backdrop;
    let flags = if level.encode { flags + ENCODED } else { flags };
    [
        x[0], x[1], x[2], taps as f32, y[0], y[1], y[2], 0., w[0], w[1], w[2], 0., target[0], target[1],
        flags, background, level.side as f32, level.opacity, level.extent[0] as f32,
        level.extent[1] as f32, texels[0], texels[1], texels[2], texels[3], r, g, b, a,
    ]
}

fn shader(device: &PipelineDevice, key: Key, scalar: bool, visibility: bool, compute: Option<bool>) -> Deferred<wgpu::ShaderModule> {
    Deferred::wgsl(device, "transform pixels", shader_source(key, scalar, visibility, compute, device.working_space()))
}
fn shader_source(key: Key, scalar: bool, visibility: bool, compute: Option<bool>, space: layer_core::color::RgbSpace) -> std::borrow::Cow<'static, str> {
    let placement = compute == Some(false);
    let prefix = format!("const scalar:bool={scalar};const visibility:bool={visibility};const placement:bool={placement};const MESH:bool={};const CLEAR=1024u;const UNMOVED=2u;\n", key.mapping == Mapping::Mesh);
    let sources = crate::texture_switch(1, TRANSFORM_SLOTS, "source_load");
    let selection = if placement {
        "fn brush_selection_at(p:vec2<f32>)->f32{return 1.;}fn horizon_selection()->f32{return 1.;}".into()
    } else {
        format!("{}\nfn horizon_selection()->f32{{return select(0.,1.,brush_selection.info.y==0u || brush_selection.info.x!=0u);}}",
            include_str!("selection_clip.wgsl").replace("@group(1) @binding(1)", "@group(1) @binding(16)"))
    };
    let mut parts = vec![prefix.as_str(), include_str!("pixel_transform/source.wgsl"), &sources, &selection];
    let mut transformed = String::new();
    if key.mapping != Mapping::Copy {
        parts.push(if key.mapping == Mapping::Affine { include_str!("pixel_transform/affine.wgsl") }
            else { include_str!("pixel_transform/projective.wgsl") });
        if key.mapping == Mapping::Mesh {
            parts.push(include_str!("pixel_transform/mesh.wgsl"));
            if key.area { parts.push(include_str!("pixel_transform/mesh_step.wgsl")); }
        }
        parts.push(match key.filter {
            0 => include_str!("pixel_transform/nearest.wgsl"), 1 => include_str!("pixel_transform/bilinear.wgsl"),
            2 => include_str!("pixel_transform/bicubic.wgsl"), 3 => include_str!("pixel_transform/lanczos.wgsl"), _ => unreachable!(),
        });
        let filter = ["nearest", "bilinear", "bicubic", "lanczos"][key.filter as usize];
        transformed.push_str(&format!("fn interpolate(s:vec2<f32>)->vec4<f32>{{return {filter}(s);}}\n"));
        if key.area {
            if key.filter != 1 { parts.push(include_str!("pixel_transform/bilinear.wgsl")); }
            parts.push(include_str!("pixel_transform/area.wgsl"));
        }
        transformed.push_str("fn transformed(world:vec2<f32>)->vec4<f32>{\n");
        if key.mapping == Mapping::Mesh {
            transformed.push_str("let p=vec2<i32>(floor(world-transform.attachment.xy))+vec2(1);let s=mesh_position(p);if s.x<=UNCOVERED{return beyond_horizon();}\n");
            transformed.push_str(if key.area { "return filtered(world,s,mesh_step(p,vec2(1,0),s),mesh_step(p,vec2(0,1),s));}" }
                else { "return interpolate(s);}" });
        } else {
            transformed.push_str("let s=source_position(world);if s.z<=0.{return beyond_horizon();}\n");
            if key.area {
                transformed.push_str(if key.mapping == Mapping::Affine { "let dx=transform.x.xy;let dy=transform.y.xy;" }
                    else { "let dx=(transform.x.xy-s.x*transform.w.xy)/s.z;let dy=(transform.y.xy-s.y*transform.w.xy)/s.z;" });
                transformed.push_str("return filtered(world,s.xy,vec2(dx.x,dy.x),vec2(dx.y,dy.y));}");
            } else { transformed.push_str("return interpolate(s.xy);}"); }
        }
    }
    parts.push(&transformed);
    if !placement { parts.push(include_str!("pixel_transform/remainder.wgsl")); }
    let pixel = if key.mapping == Mapping::Copy { "fn layer_pixel(world:vec2<f32>)->vec4<f32>{return original(vec2<i32>(floor(world)));}" }
        else if placement { "fn layer_pixel(world:vec2<f32>)->vec4<f32>{return transformed(world);}" }
        else { "fn layer_pixel(world:vec2<f32>)->vec4<f32>{if (flags()&UNMOVED)!=0u{return original(vec2<i32>(floor(world)));}return over_remainder(world,transformed(world));}" };
    parts.push(pixel);
    let output;
    let working_color = crate::working_color::source(space);
    match compute {
        None => parts.push(include_str!("pixel_transform/render.wgsl")),
        Some(false) => {
            output = include_str!("pixel_transform/placement.wgsl").replace("BINDING", if scalar { "1" } else { "0" }).replace("FORMAT", if scalar { "r32float" } else { "rgba32float" });
            parts.push(&output);
        }
        Some(true) => {parts.push(include_str!("pixel_transform/display.wgsl")); parts.push(&working_color);}
    }
    super::compose_wgsl(&parts)
}

fn source_metadata(
    bounds: [i32; 4],
    tiles: impl Iterator<Item = ([i32; 2], [u32; 2])>,
) -> [i32; (1 + TRANSFORM_SLOTS) * 4] {
    let mut metadata = [0; (1 + TRANSFORM_SLOTS) * 4];
    metadata[..4].copy_from_slice(&bounds);
    for (i, (origin, extent)) in tiles.enumerate() {
        metadata[(i + 1) * 4..(i + 2) * 4].copy_from_slice(&[
            origin[0],
            origin[1],
            extent[0] as i32,
            extent[1] as i32,
        ]);
    }
    metadata
}

fn valid_extent(origin: [i32; 2], extent: [u32; 2]) -> bool {
    origin.into_iter().zip(extent).all(|(o, n)| {
        n > 0 && i64::from(o).abs() <= 8_388_607 && (i64::from(o) + i64::from(n)).abs() <= 8_388_607
    })
}

#[cfg(test)]
mod program_tests {
    use super::*;
    #[test]
    fn all_transform_programs_validate_and_exclude_unused_algorithms() {
        for mapping in [Mapping::Copy, Mapping::Affine, Mapping::Projective, Mapping::Mesh] {
            for filter in 0..4 {
                for area in [false, true] {
                    if mapping == Mapping::Copy && (filter != 0 || area) || filter == 0 && area { continue; }
                    let key = Key { mapping, filter, area };
                    for (scalar, visibility) in [(false, false), (true, false), (true, true)] {
                        for output in [None, Some(false), Some(true)] {
                            if output == Some(true) && scalar { continue; }
                            let source = shader_source(key, scalar, visibility, output, Default::default());
                            let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{key:?}, {scalar}, {visibility}, {output:?}: {}", e.emit_to_string(&source)));
                            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                                .validate(&module).unwrap_or_else(|e| panic!("{key:?}, {scalar}, {visibility}, {output:?}: {e:?}"));
                            assert_eq!(source.contains("fn mesh_position("), mapping == Mapping::Mesh);
                            assert_eq!(source.contains("fn filtered("), area);
                            assert_eq!(source.contains("fn bicubic("), mapping != Mapping::Copy && filter == 2);
                            assert_eq!(source.contains("fn lanczos("), mapping != Mapping::Copy && filter == 3);
                            assert_eq!(source.contains("var<workgroup>"), output == Some(true));
                            assert_eq!(source.contains("var<storage, read> brush_selection"), output != Some(false));
                            assert_eq!(source.contains("s.x*transform.w.xy"), mapping == Mapping::Projective && area);
                            assert!(module.overrides.is_empty());
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn keys_follow_geometry_and_footprint_without_caching_numeric_parameters() {
        let a = ImageTransform::affine(layer_core::Affine::around(layer_core::Point { x: 50., y: 50. }, [1.; 2], 0.3, layer_core::Point { x: 0., y: 0. }));
        let b = ImageTransform::affine(layer_core::Affine::around(layer_core::Point { x: 70., y: 10. }, [1.; 2], 1.2, layer_core::Point { x: 31., y: -47. }));
        assert_eq!(Key::new(&a, 1), Key::new(&b, 1));
        assert!(!Key::new(&a, 1).area);
        assert!(Key::new(&a, 2).area);
        let mut nearest = a.clone(); nearest.placement.interpolation = Interpolation::Nearest;
        assert_eq!(Key::new(&nearest, 1), Key::new(&nearest, EXACT_TAPS));
        let mut copy = ImageTransform::default(); copy.placement.interpolation = Interpolation::Lanczos;
        assert_eq!(Key::new(&copy, EXACT_TAPS), Key::new(&Default::default(), 1));
        let mut projective = a.clone(); projective.source_from_owner = Some(Projective([1., 0., 0., 0., 1., 0., 0.001, 0., 1.]));
        assert_eq!(Key::new(&projective, EXACT_TAPS).mapping, Mapping::Projective);
    }
}
