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
    /// The page drawn at the attachment's origin.
    pub target: [u32; 2],
    pub sources: &'a [[u32; 2]],
    pub source_size: [u32; 2],
    /// x, y, width, height of the display level texels a job draws.
    pub texels: [u32; 4],
    pub unmoved: bool,
}
/// Draw a display level: each texel is the mean of `side` x `side` layer
/// pixels within `extent`, times `opacity`, over the premultiplied
/// `backdrop`. `side` divides 16.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DisplayLevel {
    pub side: u32,
    pub opacity: f32,
    pub extent: [u32; 2],
    pub backdrop: [f32; 4],
}
/// One region drawn into a shared attachment by `encode_batch`.
pub(super) struct BatchDraw<'a> {
    pub source: &'a TransformSource,
    pub job: usize,
    /// x, y, width, height in the attachment.
    pub scissor: [u32; 4],
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

pub struct PixelTransform {
    placement: bool,
    scalar: bool,
    visibility: bool,
    pub(super) pipeline: Deferred<wgpu::RenderPipeline>,
    pub(super) mesh_pipeline: Deferred<wgpu::RenderPipeline>,
    /// Color transforms drawn straight into a display level.
    pub(super) display: Option<Deferred<wgpu::ComputePipeline>>,
    display_layout: wgpu::BindGroupLayout,
    display_target: crate::bindings::CachedBinding<wgpu::TextureView>,
    layout: wgpu::BindGroupLayout,
    source_layout: wgpu::BindGroupLayout,
    empty_selection: wgpu::Buffer,
    bindings: std::collections::HashMap<SourceKey, (u64, wgpu::BindGroup)>,
    binding_frame: u64,
    uniforms: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    stride: u32,
    capacity: u64,
    records: Vec<u8>,
    next_record: u64,
}
impl PixelTransform {
    pub(super) fn staged(device: &PipelineDevice, scalar: bool) -> Self {
        Self::create(device, &shader(device), scalar, false)
    }
    /// The color, scalar and visibility passes, sharing one shader.
    pub(super) fn passes(device: &PipelineDevice) -> [Self; 3] {
        let shader = shader(device);
        [(false, false), (true, false), (true, true)]
            .map(|(scalar, visibility)| Self::create(device, &shader, scalar, visibility))
    }
    fn create(
        device: &PipelineDevice,
        shader: &Deferred<wgpu::ShaderModule>,
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
                0,
                wgpu::ShaderStages::COMPUTE,
                wgpu::TextureFormat::Rgba32Float,
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
        let pipeline = |mesh| transform_pipeline(device, &render_layout, shader, [scalar, visibility, mesh]);
        let display = (!scalar && !visibility).then(|| {
            let layout = pipeline_layout(&[Some(&layout), Some(&source_layout), Some(&display_layout)]);
            Deferred::compute(device, "transform display level", &layout, shader, "display_main")
        });
        Self {
            placement: false,
            scalar,
            visibility,
            pipeline: pipeline(false),
            mesh_pipeline: pipeline(true),
            display,
            display_layout,
            display_target: Default::default(),
            layout,
            source_layout,
            empty_selection: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("transform selects entire layer"),
                size: 48,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            bindings: Default::default(),
            binding_frame: 0,
            uniforms: None,
            stride: (RECORD_BYTES as u32)
                .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment),
            capacity: 0,
            records: Vec::new(),
            next_record: 0,
        }
    }
    /// Transactions share shader recipes. Their records remain independent.
    pub(super) fn fork(&self) -> Self {
        Self {
            placement: self.placement,
            scalar: self.scalar,
            visibility: self.visibility,
            pipeline: self.pipeline.clone(),
            mesh_pipeline: self.mesh_pipeline.clone(),
            display: self.display.clone(),
            display_layout: self.display_layout.clone(),
            display_target: Default::default(),
            layout: self.layout.clone(),
            source_layout: self.source_layout.clone(),
            empty_selection: self.empty_selection.clone(),
            bindings: Default::default(),
            binding_frame: 0,
            uniforms: None,
            stride: self.stride,
            capacity: 0,
            records: Vec::new(),
            next_record: 0,
        }
    }
    pub(super) fn placement_pass(&self) -> Self {
        let mut pass = self.fork();
        pass.placement = true;
        pass
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
        let selection = selection.unwrap_or(&self.empty_selection);
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
                &self.source_layout,
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
            (*buffer == self.empty_selection || selection.is_some_and(|s| s == buffer))
                && bound.iter().all(|v| allowed.contains(v))
        });
    }
    /// Size the records for `jobs` drawn in one frame ahead of that frame.
    pub(super) fn reserve(&mut self, device: &wgpu::Device, jobs: u64) {
        let end = jobs * u64::from(self.stride);
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
            let binding = crate::bindings::group(device, "transform records", &self.layout, [
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
        let identity = transform.is_identity();
        let stride = u64::from(self.stride);
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
                job.target.map(|v| (v * super::PAGE_SIZE) as f32),
                filter_flags(transform.interpolation)
                    + 2. * f32::from(job.unmoved || identity)
                    + 4. * f32::from(self.placement)
                    + f32::from(part as u8)
                    + 256. * f32::from(transform.keep_source),
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
        mesh: bool,
        offset: u32,
        draws: &[BatchDraw<'_>],
    ) {
        let mut pass = encoder.color_pass(
            "batched transform regions",
            attachment,
            if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load },
        );
        pass.set_pipeline(if mesh { &self.mesh_pipeline } else { &self.pipeline });
        for draw in draws {
            pass.set_bind_group(0, &self.uniforms.as_ref().unwrap().1, &[offset + draw.job as u32 * self.stride]);
            pass.set_bind_group(1, &draw.source.binding, &[]);
            let [x, y, w, h] = draw.scissor;
            pass.set_scissor_rect(x, y, w, h);
            pass.draw(0..3, 0..1);
        }
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
            crate::bindings::group(device, "transform display level", &self.display_layout, [
                wgpu::BindingResource::TextureView(level),
            ])
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("transform into display level"),
            timestamp_writes: None,
        });
        pass.set_pipeline(self.display.as_ref().expect("color transform"));
        pass.set_bind_group(2, &target, &[]);
        let texels = 16 / side;
        for draw in draws {
            pass.set_bind_group(0, &self.uniforms.as_ref().unwrap().1, &[offset + draw.job as u32 * self.stride]);
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
fn filter_flags(interpolation: Interpolation) -> f32 {
    match interpolation {
        Interpolation::Nearest => 0.,
        Interpolation::Linear => 1.,
        Interpolation::Bicubic => 9.,
        Interpolation::Lanczos => 129.,
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
    let projective = transform.map.projective().unwrap_or(Projective::IDENTITY);
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
    });
    let [r, g, b, a] = level.backdrop;
    [
        x[0], x[1], x[2], taps as f32, y[0], y[1], y[2], 0., w[0], w[1], w[2], 0., target[0], target[1],
        flags, background, level.side as f32, level.opacity, level.extent[0] as f32,
        level.extent[1] as f32, texels[0], texels[1], texels[2], texels[3], r, g, b, a,
    ]
}

fn shader(device: &PipelineDevice) -> Deferred<wgpu::ShaderModule> {
    Deferred::wgsl(device, "transform pixels", super::compose_wgsl(&[
        include_str!("pixel_transform.wgsl"),
        &crate::texture_switch(1, TRANSFORM_SLOTS, "source_load"),
        &include_str!("selection_clip.wgsl").replace("@group(1) @binding(1)", "@group(1) @binding(16)"),
    ]))
}

fn transform_pipeline(
    device: &PipelineDevice,
    layout: &wgpu::PipelineLayout,
    shader: &Deferred<wgpu::ShaderModule>,
    [scalar, visibility, mesh]: [bool; 3],
) -> Deferred<wgpu::RenderPipeline> {
    let (device, layout, shader) = (device.clone(), layout.clone(), shader.clone());
    Deferred::pipeline(move |mode| {
        let format = if scalar { device.scalar_format() } else { device.working_format() };
        super::fullscreen_pipeline_targets_with_constants_recipe(
            mode,
            &device,
            &layout,
            &shader,
            "fragment_main",
            &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            &[("scalar", f64::from(scalar)), ("visibility", f64::from(visibility)), ("mesh", f64::from(mesh))],
            "transform pixels",
        )
    })
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
