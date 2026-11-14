//! Dry pages have no cross-page dependencies: dispatch their unchanged pixel
//! evaluator together instead of opening a render pass for every page.
use super::*;

pub(super) struct Job {
    pub output: wgpu::BindGroup,
    pub source: wgpu::BindGroup,
    pub display: Option<DisplayJob>,
    pub coordinate: [u32; 2],
    pub coverage: bool,
    pub record_offset: u32,
    pub dabs: std::ops::Range<u32>,
    pub dirty: PixelRect,
}

impl Job {
    fn inputs(&self) -> &wgpu::BindGroup {
        self.display.as_ref().map_or(&self.source, |display| &display.inputs)
    }
}

/// A reduced-resolution prediction first averages its page inputs into a
/// display-input set, then evaluates against those averages.
pub(super) struct DisplayJob {
    pub gather: wgpu::BindGroup,
    pub inputs: wgpu::BindGroup,
}

pub(super) const FINE_TIP_RADIUS: f32 = 8.;
pub(super) const DISPLAY_INPUT_SETS: usize = 8;
pub(super) const AGGREGATE: u32 = 8192;
pub(super) const FINE_TIP: u32 = 16384;
pub(super) const COMPLETION: u32 = 65536;
/// Contact flags that destination-material modules are specialized for; any
/// other contact evaluates the flags each dab carries.
pub(super) const MATERIAL_CONTACTS: [u32; 3] = [0, 128, u32::MAX];
/// Brush blend modes that destination modules compile only the formula of.
pub(super) const MATERIAL_BLENDS: [BrushBlendMode; 8] = [BrushBlendMode::Normal, BrushBlendMode::Multiply, BrushBlendMode::Screen,
    BrushBlendMode::Add, BrushBlendMode::Subtract, BrushBlendMode::Darken, BrushBlendMode::Lighten, BrushBlendMode::Overlay];
pub(super) const MATERIAL_SLOTS: usize = MATERIAL_CONTACTS.len() * MATERIAL_BLENDS.len();

pub(super) fn material_contact_slot(style: &layer_render::DabStyle) -> usize {
    let flags = contact_flags(style.contact) | if style.rendering.accumulation == BrushAccumulation::Uniform { 128 } else { 0 };
    MATERIAL_CONTACTS.iter().position(|&key| key == flags).unwrap_or(MATERIAL_CONTACTS.len() - 1)
}

/// The contact and blend specialization of a style's destination modules.
pub(super) fn material_slot(style: &layer_render::DabStyle) -> usize {
    material_contact_slot(style) * MATERIAL_BLENDS.len()
        + MATERIAL_BLENDS.iter().position(|&blend| blend == style.rendering.blend_mode).unwrap()
}

/// The contact flags and blend code of a material slot.
pub(super) fn material_entries(slot: usize) -> (u32, u32) {
    let blend = layer_core::LayerBlend::from(MATERIAL_BLENDS[slot % MATERIAL_BLENDS.len()]);
    (MATERIAL_CONTACTS[slot / MATERIAL_BLENDS.len()], blend.code())
}

/// `blend` as `blend_modes.wgsl` defines it for one mode's code alone.
fn blend_source(mode: u32) -> Cow<'static, str> {
    blend_source_from(include_str!("blend_modes.wgsl"), mode)
}
fn blend_source_from(source: &str, mode: u32) -> Cow<'_, str> {
    if mode == 0 {return include_str!("blend_normal.wgsl").into();}
    let (head, cases) = source.split_once("    switch blend_mode(code) {").unwrap();
    let case = cases.split("        case ").find_map(|case| case.strip_prefix(&format!("{mode}u: "))).expect("a blend mode formula");
    format!("{head}    {}\n}}\n", case.split("        default").next().unwrap().trim()).into()
}

/// A destination-material module for one operation and contact specialization.
pub(super) fn material_shader(device: &PipelineDevice, entries: Entries, flags: u32, operation: MaterialOperation) -> Deferred<wgpu::ShaderModule> {
    let device = device.for_recipe();
    Deferred::new(move || {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer destination brush shader"),
            source: wgpu::ShaderSource::Wgsl(material_source(device.working_space(), device.float_sampling_wgsl(), entries, flags, operation)),
        })
    })
}

/// Where a dry kernel finds each texel's starting paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Start {
    /// In the output texel, which the kernel reads and updates in place, or
    /// zero for a display contribution. A prediction first stores its
    /// starting paint in its output.
    Stored,
    /// In a sampled page, for devices without read-write storage.
    Sampled,
}

fn destination(start: Start) -> Cow<'static, str> {
    match start {
        Start::Stored => compose_wgsl(&[include_str!("dry_destination.wgsl"), include_str!("changed_cells.wgsl"), include_str!("dry_read_write.wgsl")]),
        Start::Sampled => compose_wgsl(&[include_str!("dry_destination.wgsl"), include_str!("dry_write.wgsl")]),
    }
}

/// The entry points a destination-material module provides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Entries {
    /// Dry compute kernels for one blend code.
    Kernel(Start, u32),
    /// Page kernels of a transported material for one blend code, and Wet's
    /// reservoir exchange.
    Transport(u32),
    /// Smudge page kernels for one blender kernel.
    Blend(crate::destination_material::Blender),
}

pub(super) fn coverage_source(flags:u32)->Cow<'static,str> {
    let has=|bit:u32| flags==u32::MAX || flags&bit!=0;
    let fine=flags==u32::MAX || flags&FINE_TIP!=0;
    let contact=if fine {include_str!("relief_fine.wgsl")} else {include_str!("relief_coarse.wgsl")};
    let completed=flags==u32::MAX || flags&COMPLETION!=0;
    let completion=if completed {format!("{}\n{}\n{}",contact.replace("fn relief_contact(","fn relief_segment_contact("),include_str!("relief_completion.wgsl"),
        if fine {include_str!("relief_completion_fine.wgsl")} else {"fn relief_contact(dab:Dab,world:vec2<f32>,paper:f32,held:f32,required:bool)->vec2<f32> {\n    if dab.texture_sign.x!=2.0 {return relief_segment_contact(dab,world,paper,held,required);}\n    return completion_exposure(dab,world,paper);\n}"})} else {String::new()};
    let relief_contact=if completed {completion.as_str()} else {contact};
    if flags!=u32::MAX && flags&64!=0 {
        compose_wgsl(&[include_str!("brush_math.wgsl"),include_str!("paper_sample.wgsl"),include_str!("contact_field.wgsl"),
            include_str!("relief_transfer.wgsl"),include_str!("relief.wgsl"),relief_contact,include_str!("bristle_absent.wgsl"),include_str!("dry_footprint.wgsl"),include_str!("brush_alpha.wgsl")])
    } else {
        let relief=if has(64) {compose_wgsl(&[include_str!("relief_transfer.wgsl"),include_str!("relief.wgsl"),relief_contact])} else {include_str!("relief_absent.wgsl").into()};
        compose_wgsl(&[include_str!("brush_math.wgsl"),include_str!("paper_sample.wgsl"),include_str!("analytic_coverage.wgsl"),
            include_str!("brush_coverage.wgsl"),include_str!("contact_field.wgsl"),
            if has(1) {include_str!("contact.wgsl")} else {include_str!("contact_absent.wgsl")},&relief,
            if has(512) {include_str!("bristle.wgsl")} else {include_str!("bristle_absent.wgsl")},
            include_str!("brush_footprint.wgsl"),include_str!("brush_alpha.wgsl")])
    }
}

pub(super) fn specialize_contact(source: Cow<'static,str>, flags:u32)->Cow<'static,str> {
    if flags==u32::MAX {return source;}
    source.replace("override CONTACT_FLAGS: u32 = 4294967295u;",
        &format!("const CONTACT_FLAGS: u32 = {flags}u;")).into()
}

pub(super) fn material_source(space:layer_core::color::RgbSpace,float_sampling:&str,entries:Entries,flags:u32,operation:MaterialOperation)->Cow<'static,str> {
    use MaterialOperation as M;
    let kernel=matches!(entries,Entries::Kernel(..));
    let start=if let Entries::Kernel(start,_)=entries {start} else {Start::Sampled};
    let leaf=if flags==u32::MAX || flags&(64|512)==512 {include_str!("material_contact.wgsl")} else {include_str!("dry_material_contact.wgsl")};
    let clone=if operation==M::Clone {compose_wgsl(&[include_str!("retouch_sample.wgsl"),include_str!("material_clone.wgsl")])} else {include_str!("clone_absent.wgsl").into()};
    let preview = if flags == u32::MAX || flags & COMPLETION != 0 {
        include_str!("preview.wgsl")
    } else {
        include_str!("preview_absent.wgsl")
    };
    let operation_source:Cow<'static,str>=match operation {
        M::Deposit | M::Coverage | M::Clone => compose_wgsl(&[
            include_str!("material_pointwise.wgsl"),
            preview,
            "fn material_result(p:vec4<f32>)->MaterialOutput {return pointwise_material_result(p);}"]),
        _ => {
            let name=match operation {M::Liquify=>unreachable!("liquify reconstructs its original source"),M::Smudge=>"smudge",M::Wet=>"wet",_=>"watercolor"};
            let [width,height]=crate::destination_material::workgroup(operation);
            let dither=matches!(entries,Entries::Blend(blender) if blender.kind==layer_core::BlenderKind::Dither);
            let cells=matches!(entries,Entries::Blend(blender) if blender.cells);
            let scatter=matches!(entries,Entries::Blend(blender) if blender.kind==layer_core::BlenderKind::Scatter);
            let mut source=format!("const DITHER_BLENDER: bool = {dither};\nconst DITHER_CELLS: bool = {cells};\n{}\n\
                const TRANSPORT_WORKGROUP_X: u32 = {width}u;\nconst TRANSPORT_WORKGROUP_Y: u32 = {height}u;\n\
                fn operation_fragment(p:vec2<f32>,w:vec2<f32>,f:u32,c:u32)->MaterialOutput {{return {name}_fragment(p,w,f,c);}}\n\
                fn operation_color(w:vec2<f32>,r:MaterialOutput)->vec4<f32> {{return {color};}}\n",
                include_str!("material_transport.wgsl"), color=if dither {
                    "working_clamp(layer_color(dither_color(w,r),WORKING_LUMA,style.color_mode.x,style.color_mode.y))"
                } else { "r.color" });
            if let Entries::Blend(blender)=entries {
                if dither || scatter { source+=include_str!("material_cells.wgsl"); }
                if dither { source+=include_str!("material_dither.wgsl"); }
                else if scatter { source+=include_str!("material_scatter.wgsl"); }
                else { source+=&format!("const BLENDER: u32 = {}u;\nconst BLEND_PAPER: bool = {};\nconst BLEND_LIFT: bool = {};\nconst BLEND_TAPS: u32 = {}u;\n{}",
                    blender.kind as u8, blender.paper, blender.lift, if blender.paper {11} else {12}, include_str!("material_blend.wgsl"));
                }
            }
            if operation==M::Wet {source+=include_str!("material_reservoir.wgsl");}
            source.into()
        }
    };
    let entry=match entries {
        Entries::Kernel(..) => include_str!("material_compute.wgsl"),
        Entries::Blend(blender) if blender.kind == layer_core::BlenderKind::Scatter => include_str!("material_scatter_cells.wgsl"),
        Entries::Blend(blender) if blender.cells => include_str!("material_dither_cells.wgsl"),
        Entries::Transport(_) | Entries::Blend(..) => include_str!("material_destination.wgsl"),
    };
    let blend_code=match entries {Entries::Kernel(_, mode) | Entries::Transport(mode) => mode, Entries::Blend(..) => 0};
    let blend=blend_source(blend_code);
    let source=compose_wgsl(&[&working_color::source(space),include_str!("blend_codes.wgsl"),&blend,float_sampling,
        &destination(start),include_str!("brush_types.wgsl"),include_str!("brush_textures.wgsl"),&clone,
        include_str!("material_common.wgsl"),&operation_source,leaf,entry,&coverage_source(flags),
        if flags!=u32::MAX && flags&AGGREGATE!=0 {include_str!("selection_absent.wgsl")} else {include_str!("selection_clip.wgsl")}]);
    let state=matches!(entries,Entries::Blend(blender) if matches!(blender.kind, layer_core::BlenderKind::Dither | layer_core::BlenderKind::Scatter));
    let source=if state {source.replace("var material_coverage_output: texture_storage_2d<r32float, write>",
        "var material_coverage_output: texture_storage_2d<rgba32float, write>").into()} else {source};
    let absent=if kernel {0} else {64};
    let normal=blend_code==0;
    specialize_contact(source,flags)
        .replace("override MATERIAL_OPERATION: u32;",&format!("const MATERIAL_OPERATION: u32 = {}u;",operation as u32))
        .replace("override CONTACT_ABSENT: u32 = 0u;",&format!("const CONTACT_ABSENT: u32 = {absent}u;"))
        .replace("override FOOTPRINT_GRAIN: bool = true;",&format!("const FOOTPRINT_GRAIN: bool = {};",operation!=M::Smudge))
        .replace("override MATERIAL_NORMAL_BLEND: bool = false;",&format!("const MATERIAL_NORMAL_BLEND: bool = {normal};")).into()
}

/// One reduced-resolution page of averaged original paint, stroke coverage
/// and starting paint.
pub(super) struct DisplayInputs {
    views: [wgpu::TextureView; 3],
    inputs: bindings::MaterialInput,
    gather: bindings::CachedBinding<(wgpu::Buffer, wgpu::TextureView)>,
}

impl DisplayInputs {
    const FORMATS: [wgpu::TextureFormat; 3] = [wgpu::TextureFormat::Rgba32Float, wgpu::TextureFormat::R32Float, wgpu::TextureFormat::Rgba32Float];

    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            views: Self::FORMATS.map(|format| create_target(device, [PAGE_SIZE / 2; 2], format, "dry display inputs").1),
            inputs: Default::default(),
            gather: Default::default(),
        }
    }

    pub fn storage_bytes() -> u64 {
        Self::FORMATS.iter().map(|f| u64::from(f.block_copy_size(None).unwrap())).sum::<u64>() * u64::from(PAGE_SIZE / 2).pow(2)
    }

    /// The evaluator's inputs: the job's own neighborhood around the averaged
    /// page, coverage and starting paint.
    pub fn inputs(&self, r: &WgpuRasterizer, neighborhood: [&wgpu::TextureView; 9]) -> wgpu::BindGroup {
        let mut views = neighborhood;
        views[4] = &self.views[0];
        let bound = std::array::from_fn(|i| match i { 9 => &self.views[1], 10 => &self.views[2], _ => views[i] }.clone());
        self.inputs.get(([r.dab_buffer.clone(), r.material_records.binding().buffer.clone()], bound), || create_material_bind_group(
            &r.device, &r.material_layout, &views, &r.dab_buffer,
            &self.views[1], &self.views[2], wgpu::BindingResource::Buffer(r.material_records.binding())))
    }

    /// The gather pass's outputs. Where the start is stored, the averaged
    /// starting paint goes straight to the prediction's output.
    pub fn gather(&self,device:&PipelineDevice,style:&wgpu::Buffer,in_place:bool,layout:&wgpu::BindGroupLayout,output:&resident::RasterWrite<'_,PageSurface>)->wgpu::BindGroup {
        let original = if in_place { &output.value().view } else { &self.views[0] };
        self.gather.get((style.clone(), original.clone()), || {
            let entries = [wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: style, offset: 0, size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64),
                }),
            }].into_iter().chain([original, &self.views[1], &self.views[2]].into_iter().zip(4..).map(|(view, binding)| wgpu::BindGroupEntry {
                binding, resource: wgpu::BindingResource::TextureView(view),
            })).collect::<Vec<_>>();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("dry display input gather"),
                layout,
                entries: &entries,
            })
        })
    }
}

/// Dry dabs carry their color in the space they lay over paint in: the
/// document's encoded values in a Perceptual document, so the shader does not
/// convert it for every pixel.
pub(super) fn prepare_colors(style: &layer_render::DabStyle, space: layer_core::color::RgbSpace, dabs: &mut [DabGpu]) {
    if style.execution != BrushExecution::Dry
        || style.mode != DabMode::Paint
        || style.blend_space != layer_core::BlendSpace::Perceptual
    {
        return;
    }
    for dab in dabs {
        for channel in &mut dab.dab.color_rgba_linear[..3] {
            *channel = space.encode(f64::from(*channel)) as f32;
        }
    }
}

pub(super) fn prepare_geometry(style: &layer_render::DabStyle, dabs: &mut [DabGpu]) {
    let Some(dry)=style.contact.and_then(|c|c.dry) else {return};
    for d in dabs {
        let m=d.metric;
        if d.dab.is_completion() {
            let directions=d.dab.previous_contact;
            for i in 0..2 {
                let v=[-(directions[2*i]*m[0]+directions[2*i+1]*m[1]),-(directions[2*i]*m[2]+directions[2*i+1]*m[3])];
                let length=v[0].hypot(v[1]).max(1e-12);
                d.invariants[2*i]=v[0]/length;d.invariants[2*i+1]=v[1]/length;
            }
            if dry.corner==0. {
                d.dab.material[1]=dry.relief_offset*d.dab.material[3];d.dab.material[2]=0.;
            } else {
                let rim=dry.corner*d.dab.radii[0].min(d.dab.radii[1]);
                d.dab.material[1]=rim/d.dab.radii[0].max(0.005);d.dab.material[2]=rim/d.dab.radii[1].max(0.005);
            }
            continue;
        }
        let v=[d.dab.motion[0]*m[0]+d.dab.motion[1]*m[1],d.dab.motion[0]*m[2]+d.dab.motion[1]*m[3]];
        if dry.corner==0. {
            let offset = dry.relief_offset * d.dab.material[3] / (m[0]*m[0]+m[1]*m[1]);
            let tip = [m[0]*offset,m[1]*offset];
            let speed=v[0].hypot(v[1]);
            let inverse=if speed*speed>=0.00000001 {speed.recip()} else {0.};
            if speed>0. {
                let (c,s)=(v[0]/speed,v[1]/speed);
                d.metric=[c*m[0]+s*m[2],c*m[1]+s*m[3],-s*m[0]+c*m[2],-s*m[1]+c*m[3]];
            }
            d.invariants=[speed,inverse,speed*speed,inverse*inverse];
            d.dab.material[1]=d.metric[0]*tip[0]+d.metric[1]*tip[1];
            d.dab.material[2]=d.metric[2]*tip[0]+d.metric[3]*tip[1];
        } else {
            let axes=[(d.dab.radii[0]+d.dab.previous[0])*0.5,(d.dab.radii[1]+d.dab.previous[1])*0.5].map(|x|x.max(0.005));
            let rim=dry.corner*axes[0].min(axes[1]);
            d.invariants=[-v[0],-v[1],rim/axes[0],rim/axes[1]];
            let scaled=[-v[0]/d.invariants[2],-v[1]/d.invariants[3]];
            d.dab.material=[-v[0],-v[1],scaled[0]+scaled[1],scaled[0]-scaled[1]]
                .map(|v|if v.abs()<0.000001 {0.} else {v.recip()});
        }
    }
}

/// The pointwise operations with page kernels, each with and without a
/// coverage output.
const OPERATIONS: [MaterialOperation; 3] = [MaterialOperation::Deposit, MaterialOperation::Coverage, MaterialOperation::Clone];
type Kernels = [Deferred<wgpu::ComputePipeline>; 2 * OPERATIONS.len()];

#[derive(Clone)]
pub(super) struct Pipelines {
    in_place: bool,
    layouts: [wgpu::BindGroupLayout; 2],
    gather_layout: wgpu::BindGroupLayout,
    pub kernels: Kernels,
    copy: [Deferred<wgpu::ComputePipeline>; 2],
    gather: Deferred<wgpu::ComputePipeline>,
    variants: std::collections::BTreeMap<u32, Kernels>,
    blended: std::collections::BTreeMap<usize, Kernels>,
}

pub(super) fn contact_flags(contact: Option<layer_core::BrushContact>) -> u32 {
    let Some(c) = contact else {
        return 0;
    };
    1 | (u32::from(c.paper > 0.) << 1)
        | (u32::from(c.edge_roughness > 0.) << 2)
        | (u32::from(c.fiber_strength > 0.) << 3)
        | (u32::from(c.pooling > 0.) << 4)
        | (u32::from(c.depletion > 0.) << 5)
        | (u32::from(c.dry.is_some()) << 6)
        | (u32::from(c.linear_edge) << 8)
        | (u32::from(c.bristles.is_some()) << 9)
        | (u32::from(c.dry.is_some_and(|d|d.poses.iter().any(|p|p.relief[2]>0.000001))) << 11)
        | (u32::from(c.dry.is_some_and(|d|d.corner>0.)) << 12)
        | (u32::from(c.dry.is_some_and(|d|d.relief_offset!=0.)) << 15)
}

fn style_entry() -> wgpu::BindGroupLayoutEntry {
    crate::bindings::buffer(0, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Uniform, true,
        NonZeroU64::new(mem::size_of::<StyleGpu>() as u64))
}

fn storage_entry(binding: u32, format: wgpu::TextureFormat, read_write: bool) -> wgpu::BindGroupLayoutEntry {
    crate::bindings::storage_texture(binding, wgpu::ShaderStages::COMPUTE, format,
        if read_write { wgpu::StorageTextureAccess::ReadWrite } else { wgpu::StorageTextureAccess::WriteOnly })
}

/// The style and page outputs of a destination kernel: color, then stroke
/// coverage, then watercolor wetness. In-place outputs also mark changed cells.
pub(super) fn output_layouts<const N: usize>(device: &PipelineDevice, in_place: bool) -> [wgpu::BindGroupLayout; N] {
    std::array::from_fn(|scalars| {
        let mut entries = vec![style_entry(), storage_entry(1, wgpu::TextureFormat::Rgba32Float, in_place)];
        entries.extend((2..2 + scalars as u32).map(|binding| storage_entry(binding, wgpu::TextureFormat::R32Float, false)));
        if in_place {
            entries.push(crate::bindings::buffer(3, wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Storage { read_only: false }, false, NonZeroU64::new(20)));
        }
        crate::bindings::layout(device, "destination material outputs", &entries)
    })
}

pub(super) fn dither_output_layout(device: &PipelineDevice) -> wgpu::BindGroupLayout {
    crate::bindings::layout(device, "dither material outputs", &[style_entry(),
        storage_entry(1, wgpu::TextureFormat::Rgba32Float, false),
        storage_entry(2, wgpu::TextureFormat::Rgba32Float, false)])
}

/// A page output binding, retained by the coverage page when there is one.
pub(super) fn output_binding(device:&PipelineDevice,style_buffer:&wgpu::Buffer,layout:&wgpu::BindGroupLayout,
    color:&resident::RasterWrite<'_,PageSurface>,coverage:Option<&resident::RasterWrite<'_,PageSurface>>,
    wetness:Option<&resident::RasterWrite<'_,PageSurface>>,changed:Option<&wgpu::Buffer>)->wgpu::BindGroup {
    let color=color.value();let coverage=coverage.map(|write|write.value());let wetness=wetness.map(|write|write.value());
    let style = wgpu::BindingResource::Buffer(wgpu::BufferBinding {
        buffer: style_buffer, offset: 0, size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64),
    });
    let entries = [(0, Some(style)), (1, Some(wgpu::BindingResource::TextureView(&color.view))),
        (2, coverage.map(|page| wgpu::BindingResource::TextureView(&page.view))),
        (3, wetness.map(|page| wgpu::BindingResource::TextureView(&page.view))), (3, changed.map(wgpu::Buffer::as_entire_binding))]
        .into_iter().filter_map(|(binding, resource)| resource.map(|resource| wgpu::BindGroupEntry { binding, resource }))
        .collect::<Vec<_>>();
    coverage.unwrap_or(color).material_output.get(
        (style_buffer.clone(), color.view.clone(), [coverage, wetness].map(|p| p.map(|p| p.view.clone())), changed.cloned()),
        || device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("destination material page output"), layout, entries: &entries }),
    )
}

impl Pipelines {
    /// `shared` holds the target, material source and texture layouts;
    /// `outputs` the write-only output layouts.
    pub fn new(device: &PipelineDevice, shared: [&wgpu::BindGroupLayout; 3], in_place: bool, outputs: &[wgpu::BindGroupLayout; 3]) -> Self {
        let layouts = if in_place { output_layouts(device, true) } else { [outputs[0].clone(), outputs[1].clone()] };
        let gather_layout = crate::bindings::layout(device, "dry display inputs", &[style_entry(),
            storage_entry(4, wgpu::TextureFormat::Rgba32Float, false), storage_entry(5, wgpu::TextureFormat::R32Float, false),
            storage_entry(6, wgpu::TextureFormat::Rgba32Float, false)]);
        let pipeline_layout = |group: &wgpu::BindGroupLayout| device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dry material pages"),
            bind_group_layouts: &[Some(group), Some(shared[0]), Some(shared[1]), Some(shared[2])],
            immediate_size: 0,
        });
        let pipeline_layouts: [_; 2] = std::array::from_fn(|coverage| pipeline_layout(&layouts[coverage]));
        let start = if in_place { Start::Stored } else { Start::Sampled };
        let make_kernels = |flags: u32, blend: u32| -> Kernels {
            let shaders=OPERATIONS.map(|operation| {
                let device=device.for_recipe();
                Deferred::new(move||device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label:Some("layer specialized contact shader"),
                    source:wgpu::ShaderSource::Wgsl(material_source(device.working_space(),device.float_sampling_wgsl(),Entries::Kernel(start,blend),flags,operation)),
                }))
            });
            std::array::from_fn(|index| {
                let entry = if index % 2 == 0 { "compute_color" } else { "compute_coverage" };
                Deferred::compute(device, format!("dry material {entry} operation={} contact={flags:#x} blend={blend}", OPERATIONS[index / 2] as u32),
                    &pipeline_layouts[index % 2], &shaders[index / 2], entry)
            })
        };
        let pages = Deferred::wgsl(device, "dry material pages",
            material_source(device.working_space(), device.float_sampling_wgsl(), Entries::Kernel(start, 0), u32::MAX, MaterialOperation::Deposit));
        let copy = std::array::from_fn(|coverage| Deferred::compute(device, "dry unchanged pixels", &pipeline_layouts[coverage], &pages, "copy_color"));
        let gather = Deferred::compute(device, "dry display inputs", &pipeline_layout(&gather_layout), &pages, "gather_display");
        let kernels = make_kernels(u32::MAX, 0);
        let retouching = [
            layer_core::DefaultBrushPreset::CloneStamp,
            layer_core::DefaultBrushPreset::HealingBrush,
            layer_core::DefaultBrushPreset::SpotHealingBrush,
        ];
        let mut flags = layer_core::CONTACT_BRUSH_PRESETS
            .into_iter()
            .chain(retouching)
            .map(|p| contact_flags(layer_core::default_brush(p).contact))
            .collect::<std::collections::BTreeSet<_>>();
        for paper in [0,2] {for quartic in [0,2048] {for corner in [0,4096] {
            flags.insert(1|64|paper|quartic|corner);
        }}}
        flags.extend(flags.clone().into_iter().filter(|f|f&64!=0 && f&4096==0).map(|f|f|32768));
        flags.insert(0);
        flags.extend(flags.clone().into_iter().map(|flags| flags | 128));
        flags.extend(flags.clone().into_iter().filter(|f|f&64!=0 && f&128==0).map(|f|f|AGGREGATE));
        flags.extend(flags.clone().into_iter().filter(|f|f&64!=0 && f&128==0).map(|f|f|COMPLETION));
        flags.extend(flags.clone().into_iter().filter(|f|f&64!=0 && (f&4096==0 || f&COMPLETION!=0)).map(|f|f|FINE_TIP));
        let variants = flags.into_iter().map(|f| (f, make_kernels(f, 0))).collect();
        let blended = (0..MATERIAL_SLOTS).filter(|slot| slot % MATERIAL_BLENDS.len() != 0).map(|slot| {
            let (flags, blend) = material_entries(slot);
            (slot, make_kernels(flags, blend))
        }).collect();
        Self { in_place, layouts, gather_layout, kernels, copy, gather, variants, blended }
    }

    pub(super) fn in_place(&self) -> bool {
        self.in_place
    }

    pub(super) fn copy_kernel(&self, coverage: bool) -> &Deferred<wgpu::ComputePipeline> {
        &self.copy[usize::from(coverage)]
    }

    pub(super) fn gather_kernel(&self) -> &Deferred<wgpu::ComputePipeline> {
        &self.gather
    }

    pub(super) fn selected_kernel(&self,style:&layer_render::DabStyle,operation:MaterialOperation,coverage:bool,extra:u32)->&Deferred<wgpu::ComputePipeline> {
        // Flow integration and maximum-film deposition have different kernels.
        // Resolve that uniform branch at compilation, including its register
        // requirements, rather than carrying both models through every pixel.
        let flags = extra | contact_flags(style.contact)
            | if style.rendering.accumulation == BrushAccumulation::Uniform { 128 } else { 0 };
        let kernels = if style.rendering.blend_mode == BrushBlendMode::Normal { self.variants.get(&flags).unwrap_or(&self.kernels) }
            else { &self.blended[&material_slot(style)] };
        let slot = OPERATIONS.iter().position(|o| *o == operation).expect("a pointwise operation");
        &kernels[slot * 2 + usize::from(coverage)]
    }

    pub(super) fn dispatch_kernel(&self, style: &layer_render::DabStyle, operation: MaterialOperation, coverage: bool, extra: u32) -> &Deferred<wgpu::ComputePipeline> {
        let preferred = self.selected_kernel(style, operation, coverage, extra);
        if preferred.ready() { return preferred; }
        let class = self.selected_kernel(style, operation, coverage, class_flags(style, extra));
        if class.ready() { class }
        else { self.selected_kernel(style, operation, coverage, superset_flags(style, extra)) }
    }

    /// Changed cells are marked only for a tracked page; every other in-place
    /// destination binds the disabled set.
    pub fn gather_layout(&self)->&wgpu::BindGroupLayout {&self.gather_layout}
    pub fn output(&self,device:&PipelineDevice,style:&wgpu::Buffer,color:&resident::RasterWrite<'_,PageSurface>,
        coverage:Option<&resident::RasterWrite<'_,PageSurface>>,changed:Option<&wgpu::Buffer>)->wgpu::BindGroup {
        output_binding(device,style,&self.layouts[usize::from(coverage.is_some())],color,coverage,None,changed)
    }

}

impl WgpuRasterizer {
    pub(super) fn dry_display(&self, batch: &DabBatch) -> bool {
        batch.kind == DabBatchKind::Preview && (self.preview_level > 0 || self.compact_preview_contribution(batch))
    }

    pub(super) fn in_place_dry_material(&self, batch: &DabBatch) -> bool {
        batch.kind == DabBatchKind::Persistent && pointwise(&batch.style)
            && BrushPassPlan::for_device(&batch.style, &self.device).requires_destination()
            && self.pipelines.dry_material.in_place()
    }

    /// Display blocks average their inputs in a separate pass. A contribution
    /// with no stroke coverage has only zero inputs and skips it.
    pub(super) fn dry_display_gathers(&self, batch: &DabBatch) -> bool {
        self.dry_display(batch) && self.dry_material_block(batch) > 1
            && (!self.compact_preview_contribution(batch) || BrushPassPlan::for_device(&batch.style, &self.device).state.coverage)
    }

    pub(super) fn dry_material_block(&self, batch: &DabBatch) -> u32 {
        if batch.kind == DabBatchKind::Preview {
            if self.preview_level > 0 { 1 << self.preview_level } else { 1 }
        } else { 1 }
    }

    pub(super) fn compact_preview_contribution(&self, batch: &DabBatch) -> bool {
        batch.kind == DabBatchKind::Preview && self.preview_contribution
            && batch.style.mode == DabMode::Paint && batch.style.opacity == 1.
            && ((self.preview_level > 0 && display_preview_eligible(&batch.style)) || native_dry_preview_eligible(&batch.style))
    }

    pub(super) fn encode_dry_material_jobs(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        jobs: &[Job],
    ) {
        if jobs.is_empty() {
            return;
        }
        let operation = BrushPassPlan::for_device(&batch.style, &self.device).material;
        let pipelines = &self.pipelines.dry_material;
        let in_place = self.in_place_dry_material(batch);
        crate::performance_trace::counter(c"Capy dry material in place", u64::from(in_place));
        let texture_key = Self::texture_set_key(&batch.style);
        let textures = self
            .texture_sets
            .iter()
            .find(|set| set.key == texture_key)
            .unwrap();
        let style_offset = batch_index as u32 * self.style_stride as u32;
        let block = self.dry_material_block(batch);
        let display_size = [PAGE_SIZE.div_ceil(block).div_ceil(32), PAGE_SIZE.div_ceil(block).div_ceil(2), 1];
        if jobs.iter().any(|job| job.display.is_some()) {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("dry material display inputs"), timestamp_writes: None,
            });
            pass.set_bind_group(3, &textures.bind_group, &[]);
            pass.set_pipeline(pipelines.gather_kernel());
            for job in jobs {
                let Some(display) = &job.display else { continue };
                pass.set_bind_group(0, &display.gather, &[style_offset]);
                pass.set_bind_group(1, self.paint_target_binding(&batch.style), &[self.layer_target_offset(batch.target, job.coordinate)]);
                pass.set_bind_group(2, &job.source, &[job.record_offset]);
                pass.dispatch_workgroups(display_size[0], display_size[1], display_size[2]);
            }
        }
        let cropped = operation == MaterialOperation::Deposit && dry_flow(&batch.style);
        let stored_start = pipelines.in_place() && !self.compact_preview_contribution(batch);
        let copied = |job: &&Job| !in_place && if stored_start { job.display.is_none() } else { !job.coverage && cropped };
        if jobs.iter().any(|job| copied(&job)) {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("dry material copy pages"), timestamp_writes: None,
            });
            pass.set_bind_group(3, &textures.bind_group, &[]);
            for job in jobs.iter().filter(copied) {
                pass.set_pipeline(pipelines.copy_kernel(job.coverage));
                pass.set_bind_group(0, &job.output, &[style_offset]);
                pass.set_bind_group(1, self.paint_target_binding(&batch.style), &[self.layer_target_offset(batch.target, job.coordinate)]);
                pass.set_bind_group(2, job.inputs(), &[job.record_offset]);
                pass.dispatch_workgroups(display_size[0], display_size[1], display_size[2]);
            }
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("dry material pages"),
            timestamp_writes: None,
        });
        pass.set_bind_group(3, &textures.bind_group, &[]);
        let marker=batch_index*self.style_stride as usize+mem::offset_of!(StyleGpu,material_a)+12;
        let aggregate=f32::from_ne_bytes(self.style_upload[marker..marker+4].try_into().unwrap())>0.5
            && aggregate_kernel(&batch.style);
        let owner = if batch.kind == DabBatchKind::Preview && dry_flow(&batch.style) {
            batch
                .preview_end
                .and_then(|i| self.dab_upload.get((batch.first_dab + i) as usize))
                .map(|d| d.dab.bounds())
        } else {
            None
        };
        for job in jobs {
            if job.dabs.is_empty() {
                continue;
            }
            let dabs = &self.dab_upload[job.dabs.start as usize..job.dabs.end as usize];
            let owns = owner.is_some_and(|end| {
                !crate::brush_tiles::plain_preview_page(Some(end), job.coordinate)
            });
            let extra = if aggregate { AGGREGATE } else { 0 }
                | batch_contact_flags(&batch.style, dabs, owns);
            pass.set_bind_group(0, &job.output, &[style_offset]);
            pass.set_bind_group(
                1,
                self.paint_target_binding(&batch.style),
                &[self.layer_target_offset(batch.target, job.coordinate)],
            );
            pass.set_bind_group(2, job.inputs(), &[job.record_offset]);
            let cropped=!job.coverage && cropped;
            let kernel = pipelines.dispatch_kernel(&batch.style, operation, job.coverage, extra);
            debug_assert!(kernel.ready() || !self.startup.as_ref().is_some_and(|s| s.strokes.covers(&batch.style)), "dry kernel {extra:#x} was not prepared for its brush");
            pass.set_pipeline(kernel);
            let dirty=job.dirty;
            let size=if cropped {[dirty.max_x().div_ceil(block)-dirty.min_x()/block,
                dirty.max_y().div_ceil(block)-dirty.min_y()/block]} else {[PAGE_SIZE.div_ceil(block);2]};
            pass.dispatch_workgroups(size[0].div_ceil(32),size[1].div_ceil(2),1);
        }
    }
}

pub(super) fn single_pigment_marked(style: &layer_render::DabStyle) -> bool {
    style.execution == BrushExecution::Dry && !style.contact.is_some_and(|c| c.bristles.is_some())
        && style.rendering.blend_mode == BrushBlendMode::Normal && !style.alpha_locked && style.selection.is_none()
}

pub(super) fn dry_flow(style: &layer_render::DabStyle) -> bool {
    style.contact.is_some_and(|c| c.dry.is_some()) && style.rendering.accumulation == BrushAccumulation::Flow
}

fn batch_contact_flags(style: &layer_render::DabStyle, dabs: &[DabGpu], owns: bool) -> u32 {
    let completion = dry_flow(style)
        && dabs
            .iter()
            .any(|d| d.dab.is_completion() || (owns && matches!(d.dab.texture_sign[0], -2. | -3.)));
    let fine = (fine_tip_capable(style) || completion)
        && dabs
            .iter()
            .any(|d| (0..2).any(|i| 0.5 * (d.dab.radii[i] + d.dab.previous[i]) < FINE_TIP_RADIUS));
    (if fine { FINE_TIP } else { 0 }) | if completion { COMPLETION } else { 0 }
}

fn aggregate_kernel(style: &layer_render::DabStyle) -> bool {
    single_pigment_marked(style) && dry_flow(style)
}

fn fine_tip_capable(style: &layer_render::DabStyle) -> bool {
    style.contact.is_some_and(|c| c.dry.is_some_and(|d| d.corner == 0.))
}

fn superset_flags(style: &layer_render::DabStyle, extra: u32) -> u32 {
    if style.execution != BrushExecution::Dry || !style.contact.is_some_and(|c| c.dry.is_some()) { return extra; }
    extra & !(FINE_TIP | COMPLETION)
        | if fine_tip_capable(style) { FINE_TIP } else { 0 }
        | if dry_flow(style) { COMPLETION } else { 0 }
}

fn class_flags(style: &layer_render::DabStyle, extra: u32) -> u32 {
    superset_flags(style, extra) & if extra & FINE_TIP == 0 { !FINE_TIP } else { !0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Reach {
    coarse_tip: bool,
    varying_color: bool,
}

impl Reach {
    pub fn of(brush: &layer_core::BrushSnapshot) -> Self {
        Self {
            coarse_tip: brush.contact.and_then(|c| c.dry).is_none_or(|d| d.corner > 0.)
                || brush.conservative_minor_radius() >= FINE_TIP_RADIUS,
            varying_color: brush.dab_color_varies(),
        }
    }

    pub fn of_dabs(dabs: &[layer_render::Dab]) -> Self {
        Self {
            coarse_tip: !dabs.is_empty() && !dabs.iter().any(|dab| (0..2).any(|i| 0.5 * (dab.radii[i] + dab.previous[i]) < FINE_TIP_RADIUS)),
            varying_color: dabs.iter().any(|dab| dab.color_rgba_linear[..3] != dabs[0].color_rgba_linear[..3]),
        }
    }

    pub fn any_size(self) -> Self {
        Self { coarse_tip: true, ..self }
    }

    pub fn batch_variants(self, style: &layer_render::DabStyle) -> impl Iterator<Item = u32> {
        self.variants(style, false)
    }

    pub fn required_variants(self, style: &layer_render::DabStyle) -> impl Iterator<Item = u32> {
        self.variants(style, true)
    }

    fn variants(self, style: &layer_render::DabStyle, required: bool) -> impl Iterator<Item = u32> {
        let aggregate = aggregate_kernel(style);
        let pigments = [aggregate.then_some(AGGREGATE),
            (!aggregate || (style.mode == DabMode::Paint && self.varying_color)).then_some(0)];
        let fine = fine_tip_capable(style);
        let tips = if required && style.execution == BrushExecution::Dry && style.contact.is_some_and(|c| c.dry.is_some()) {
            [fine.then(|| class_flags(style, FINE_TIP)), (!fine || self.coarse_tip).then(|| class_flags(style, 0)), None, None]
        } else {
            [fine.then_some(FINE_TIP), (!fine || self.coarse_tip).then_some(0),
                dry_flow(style).then_some(COMPLETION | FINE_TIP), (dry_flow(style) && self.coarse_tip).then_some(COMPLETION)]
        };
        pigments.into_iter().flatten().flat_map(move |pigment| tips.into_iter().flatten().map(move |tip| pigment | tip))
    }
}

/// Basic analytic contacts (including G-Pen, eraser and airbrush) have no
/// unresolved paper, fiber, bristle or texture frequencies at display scale.
pub(super) fn display_preview_eligible(style: &layer_render::DabStyle) -> bool {
    style.execution == BrushExecution::Dry && style.rendering.blend_mode == BrushBlendMode::Normal && !style.rendering.edge_after_stroke
        && !style.alpha_locked && style.selection.is_none() && style.grain.is_none()
        && matches!(style.tip, BrushTip::AnalyticEllipse) && contact_flags(style.contact) <= 1
}

pub(super) fn native_dry_preview_eligible(style: &layer_render::DabStyle) -> bool {
    style.execution == BrushExecution::Dry && style.opacity == 1. && style.rendering.blend_mode == BrushBlendMode::Normal
        && style.contact.is_some_and(|c| c.dry.is_some()) && contact_flags(style.contact) & !(67|2048|4096|32768) == 0
        && style.rendering.accumulation == BrushAccumulation::Flow && !style.rendering.edge_after_stroke
        && !style.alpha_locked && style.selection.is_none()
}

/// Predictions of these contacts evaluate at the reduced display level when
/// the view is zoomed out; the committed stroke replaces them at full resolution.
pub(super) fn reduced_preview_eligible(style: &layer_render::DabStyle) -> bool {
    display_preview_eligible(style) || native_dry_preview_eligible(style)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_superset_kernels_match_exact_body_and_preview_bytes() {
        use crate::{layer_tests::{dab, page_bytes, preset_style}, test_support::{dab_batch, packet}};
        use layer_core::{BrushAccumulation::{Flow, Uniform}, DefaultBrushPreset::{Pencil, PointyPencil, Charcoal, PastelBlock},
            Document, DocumentNames, Point, PortableId, Rect};
        use layer_render::DabPreview;

        #[derive(Clone, Copy, Debug)]
        struct Case {
            name: &'static str,
            preset: layer_core::DefaultBrushPreset,
            accumulation: BrushAccumulation,
            radius: f32,
            moving: bool,
            mixed: bool,
            coverage: bool,
            opacity: f32,
            preview: bool,
            ownership: bool,
        }
        let base = Case { name: "round-flow-small-moving", preset: Pencil, accumulation: Flow,
            radius: 3., moving: true, mixed: false, coverage: false, opacity: 1., preview: false, ownership: false };
        let cases = [
            base,
            Case { name: "round-flow-small-tap", moving: false, ..base },
            Case { name: "round-flow-blend-moving", radius: 6., ..base },
            Case { name: "round-flow-blend-tap", radius: 6., moving: false, ..base },
            Case { name: "round-flow-coarse-moving", radius: 12., ..base },
            Case { name: "round-flow-coarse-tap", radius: 12., moving: false, ..base },
            Case { name: "round-flow-mixed-small", mixed: true, ..base },
            Case { name: "round-flow-mixed-coarse", mixed: true, radius: 12., ..base },
            Case { name: "pointy-flow-small", preset: PointyPencil, ..base },
            Case { name: "pointy-flow-coarse", preset: PointyPencil, radius: 12., ..base },
            Case { name: "pointy-flow-preview-owner", preset: PointyPencil, preview: true, ownership: true, radius: 6., ..base },
            Case { name: "charcoal-flow-small", preset: Charcoal, ..base },
            Case { name: "charcoal-flow-coarse", preset: Charcoal, radius: 12., ..base },
            Case { name: "round-flow-grouped", opacity: 0.55, radius: 6., ..base },
            Case { name: "round-flow-coverage", coverage: true, radius: 12., ..base },
            Case { name: "round-flow-mixed-coverage", mixed: true, coverage: true, radius: 6., ..base },
            Case { name: "rounded-flow-moving", preset: PastelBlock, ..base },
            Case { name: "rounded-flow-tap", preset: PastelBlock, moving: false, radius: 12., ..base },
            Case { name: "rounded-flow-mixed", preset: PastelBlock, mixed: true, radius: 6., ..base },
            Case { name: "round-uniform-small", accumulation: Uniform, ..base },
            Case { name: "round-uniform-blend-tap", accumulation: Uniform, radius: 6., moving: false, ..base },
            Case { name: "round-uniform-coarse", accumulation: Uniform, radius: 12., ..base },
            Case { name: "round-uniform-grouped", accumulation: Uniform, opacity: 0.55, ..base },
            Case { name: "round-flow-preview-body", preview: true, radius: 6., ..base },
            Case { name: "round-flow-preview-owner-small", preview: true, ownership: true, ..base },
            Case { name: "round-flow-preview-owner-coarse", preview: true, ownership: true, radius: 12., ..base },
            Case { name: "round-flow-preview-owner-mixed", preview: true, ownership: true, mixed: true, radius: 6., ..base },
            Case { name: "rounded-flow-preview-owner", preset: PastelBlock, preview: true, ownership: true, radius: 6., ..base },
            Case { name: "round-uniform-preview", accumulation: Uniform, preview: true, radius: 6., ..base },
            Case { name: "round-flow-preview-coverage", preview: true, coverage: true, radius: 12., ..base },
        ];
        let extent = [768, 256];
        let document = Document::new(PortableId::random(), extent[0], extent[1],
            DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let target = document.working.target.unwrap();
        for case in cases {
            let mut style = preset_style(case.preset);
            style.opacity = case.opacity;
            style.rendering.accumulation = case.accumulation;
            style.rendering.edge_after_stroke = case.coverage;
            style.rendering.wet_edge = if case.coverage { 0.1 } else { 0. };
            let dry = style.contact.unwrap().dry.unwrap();
            let rounded = matches!(case.preset, PastelBlock);
            assert_eq!(dry.corner > 0., rounded, "{}", case.name);
            assert_eq!(contact_flags(style.contact) & 32768 != 0, case.preset == PointyPencil, "{case:?}");
            let relief = dry.pose(0.5).relief;
            let body = Dab {
                center: Point { x: 128., y: 128. },
                radii: [case.radius * 1.2, case.radius],
                previous: [case.radius * 1.2, case.radius, 0.8, 0.6],
                rotation: [0.8, 0.6],
                motion: if case.moving { [11., 7.] } else { [0.; 2] },
                contact: [0.7, relief[0], relief[1], relief[2]],
                previous_contact: [0.45, relief[0], relief[1], relief[2]],
                flow: 0.17,
                material: [0., 0., 0., 0.65],
                ..dab([0.12, 0.18, 0.24, 0.8])
            };
            let mut overlap = body;
            overlap.center.x += 2.;
            overlap.contact[0] = 0.9;
            overlap.previous_contact[0] = 0.6;
            if case.mixed { overlap.color_rgba_linear = [0.65, 0.08, 0.18, 0.8]; }
            let mut dabs = vec![body, overlap];
            let mut batch = dab_batch(target, style.clone(), Rect::from_extent(extent));
            batch.kind = if case.preview { DabBatchKind::Preview } else { DabBatchKind::Persistent };
            batch.stroke_end = false;
            if case.ownership {
                let mut main = overlap;
                main.center.x = 600.;
                main.motion = [510., 0.];
                let end = main.completion([-1., 0.], [0.; 2], 1.);
                dabs.extend([main.with_preview_tag(DabPreview::LiftOnly), end,
                    main.with_preview_tag(DabPreview::MainOnly)]);
                batch.preview_end = Some(3);
            }
            batch.dab_count = dabs.len() as u32;
            let pigment = if aggregate_kernel(&style) && !case.mixed { AGGREGATE } else { 0 };
            let frame = FramePacket { dabs: &dabs, dab_batches: std::slice::from_ref(&batch),
                ..packet(document.scene(), extent) };
            let tiles = crate::brush_tiles::plan_packet(frame, |_| extent).unwrap().remove(0);
            assert!(!tiles.is_empty(), "{case:?}");
            let end = crate::brush_tiles::preview_end_bounds(&batch, &dabs);
            let mut extras = std::collections::BTreeSet::new();
            let mut plain = false;
            let mut owned = false;
            for tile in &tiles {
                let indices = if tile.indices.is_empty() { tile.dabs.clone().collect::<Vec<_>>() }
                    else { tile.indices.clone() };
                let mut prepared = indices.iter().map(|&i| dabs[i as usize].into()).collect::<Vec<DabGpu>>();
                prepare_geometry(&style, &mut prepared);
                let owns = end.is_some_and(|end| !crate::brush_tiles::plain_preview_page(Some(end), tile.coordinate));
                let extra = pigment | batch_contact_flags(&style, &prepared, owns);
                extras.insert(extra);
                if case.ownership && !owns {
                    plain = true;
                    assert!(indices.iter().all(|&i| dabs[i as usize].preview_tag() != DabPreview::LiftOnly), "{case:?}");
                    assert_eq!(extra & COMPLETION, 0, "{case:?}: a plain page selects a body kernel");
                }
                if owns {
                    owned = true;
                    assert_ne!(extra & COMPLETION, 0, "{case:?}: the owner selects a completion kernel");
                }
            }
            assert!(!case.ownership || (plain && owned), "{case:?}: both ownership routes must execute");
            for write_only in [false, true] {
                let render = |route: fn(&layer_render::DabStyle, u32) -> u32| {
                    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
                    if write_only { crate::test_support::write_only_dry_material(&mut r); }
                    let plan = BrushPassPlan::for_device(&style, &r.device);
                    assert!(plan.direct.is_none(), "{case:?}");
                    assert_eq!(plan.state.coverage, case.coverage || case.accumulation == Uniform && case.opacity == 1., "{case:?}");
                    let slot = OPERATIONS.iter().position(|&operation| operation == plan.material).unwrap() * 2;
                    let base_flags = contact_flags(style.contact) | if case.accumulation == Uniform { 128 } else { 0 };
                    let mut identities = Vec::new();
                    for &extra in &extras {
                        for coverage in [false, true].into_iter().filter(|&coverage| !coverage || plan.state.coverage) {
                            let p = &r.pipelines.dry_material;
                            let selected_extra = route(&style, extra);
                            let expected = p.selected_kernel(&style, plan.material, coverage, selected_extra).clone();
                            if selected_extra != extra {
                                Arc::make_mut(&mut r.pipelines).dry_material.variants.get_mut(&(base_flags | extra))
                                    .expect("a specialized exact recipe")[slot + usize::from(coverage)] = expected.clone();
                            }
                            let selected = r.pipelines.dry_material.selected_kernel(&style, plan.material, coverage, extra);
                            pollster::block_on(r.compilation.prepare_all([selected,&expected])).unwrap();
                            assert!(std::ptr::eq(selected.compile(), expected.compile()),
                                "{case:?} write_only={write_only} extra={extra:#x} coverage={coverage}: selected recipe identity");
                            identities.push((extra, coverage, expected));
                        }
                    }
                    crate::test_support::submit_ready(&mut r, FramePacket { dabs: &[], dab_batches: &[], ..frame }).unwrap();
                    let blank = r.readback_srgb_rgba8().unwrap();
                    crate::test_support::submit_ready(&mut r, FramePacket { reset_layers: false, ..frame }).unwrap();
                    let rgba = r.readback_srgb_rgba8().unwrap();
                    assert!(rgba != blank, "{case:?} write_only={write_only}: composed paint must be visible");
                    for (extra, coverage, expected) in identities {
                        let selected = r.pipelines.dry_material.selected_kernel(&style, plan.material, coverage, extra);
                        assert!(std::ptr::eq(selected.compile(), expected.compile()), "{case:?}: recipe identity after encoding");
                    }
                    let layer = r.paint_layers.iter().find(|layer| layer.id == target).unwrap();
                    let mut color = layer.pages.iter().map(|p| {
                        let expected=r.current_raster_version(target,RasterPlane::Color,p.coordinate).unwrap();
                        (p.coordinate,page_bytes(&r,&p.active().read(expected,None).unwrap().value().texture))
                    }).collect::<Vec<_>>();
                    let mut coverage = layer.coverage_pages.iter().map(|p| {
                        let input=r.raster_intent(target,RasterPlane::Color,p.coordinate).unwrap();
                        let state=p.state.as_ref().filter(|state|state.matches(batch.stroke_id,&input)).unwrap();
                        (p.coordinate,page_bytes(&r,&p.active().read(&state.version,None).unwrap().value().texture))
                    }).collect::<Vec<_>>();
                    let mut preview = r.preview_pages.iter().map(|p| {
                        let expected=r.preview_expected(target,RasterPlane::Color,p.coordinate,scene::PreviewStage::Final).unwrap();
                        (p.coordinate,page_bytes(&r,&p.active().read(expected,None).unwrap().value().texture))
                    }).collect::<Vec<_>>();
                    let mut preview_coverage = r.preview_coverage_pages.iter().map(|p| {
                        let expected=r.preview_pass_expected(target,RasterPlane::Color,p.coordinate,scene::PreviewStage::Final,scene::PreviewPass::Coverage).unwrap();
                        (p.coordinate,page_bytes(&r,&p.active().read(expected,None).unwrap().value().texture))
                    }).collect::<Vec<_>>();
                    for pages in [&mut color, &mut coverage, &mut preview, &mut preview_coverage] { pages.sort_by_key(|(coordinate, _)| *coordinate); }
                    let painted = if case.preview { &preview } else { &color };
                    assert!(painted.iter().any(|(_, bytes)| bytes.chunks_exact(16)
                        .any(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()) > 0.)), "{case:?}: raw paint must be nonempty");
                    if plan.state.coverage {
                        assert_eq!(plan.material, MaterialOperation::Coverage, "{case:?}");
                        if case.preview {
                            assert!(coverage.is_empty() && preview_coverage.is_empty(),
                                "{case:?}: a single preview reads persistent paint and writes color");
                        } else {
                            assert!(layer.coverage_pages.iter().any(|p| p.owner == Some(batch.stroke_id)),
                                "{case:?}: the coverage entry must own a destination");
                            assert!(coverage.iter().any(|(_, bytes)| bytes.chunks_exact(4)
                                .any(|p| f32::from_le_bytes(p.try_into().unwrap()) > 0.)),
                                "{case:?}: the coverage entry must write coverage");
                        }
                    }
                    (color, coverage, preview, preview_coverage, rgba)
                };
                let exact = render(|_, extra| extra);
                for (name, route) in [("full", superset_flags as fn(&layer_render::DabStyle, u32) -> u32), ("tip class", class_flags)] {
                    let routed = render(route);
                    assert!(exact.0 == routed.0, "{case:?} write_only={write_only} {name}: persistent color bytes differ");
                    assert!(exact.1 == routed.1, "{case:?} write_only={write_only} {name}: persistent coverage bytes differ");
                    assert!(exact.2 == routed.2, "{case:?} write_only={write_only} {name}: preview color bytes differ");
                    assert!(exact.3 == routed.3, "{case:?} write_only={write_only} {name}: preview coverage bytes differ");
                    assert!(exact.4 == routed.4, "{case:?} write_only={write_only} {name}: composed RGBA bytes differ");
                }
            }
        }
    }

    #[test]
    fn completion_preview_routes_select_the_committed_filter() {
        let body = Dab {
            center: layer_core::Point::default(),
            radii: [4.; 2],
            rotation: [1., 0.],
            motion: [1., 0.],
            color_rgba_linear: [1.; 4],
            flow: 1.,
            hardness: 1.,
            texture_sign: [1.; 2],
            material: [0.; 4],
            previous: [4., 4., 1., 0.],
            contact: [0.5, 0., 0.7, 0.],
            previous_contact: [0.5, 0., 0.7, 0.],
        };
        let completion = body.completion([-1., 0.], [0.; 2], 1.);
        let style = |preset| {
            layer_render::DabStyle::for_brush(
                &layer_core::default_brush(preset),
                layer_core::StrokeTool::Brush,
            )
        };
        let pastel = style(layer_core::DefaultBrushPreset::PastelBlock);
        assert_eq!(
            batch_contact_flags(&pastel, &[body.into(), completion.into()], false),
            FINE_TIP | COMPLETION
        );
        let pencil = style(layer_core::DefaultBrushPreset::Pencil);
        assert_eq!(
            batch_contact_flags(&pencil, &[body.into(), completion.into()], false),
            FINE_TIP | COMPLETION
        );
        for tag in [
            layer_render::DabPreview::LiftOnly,
            layer_render::DabPreview::MainOnly,
        ] {
            let tagged = [body.with_preview_tag(tag).into()];
            assert_eq!(batch_contact_flags(&pencil, &tagged, false), FINE_TIP);
            assert_eq!(
                batch_contact_flags(&pencil, &tagged, true),
                FINE_TIP | COMPLETION
            );
        }
        for preset in [
            layer_core::DefaultBrushPreset::PastelBlock,
            layer_core::DefaultBrushPreset::Pencil,
        ] {
            let brush = layer_core::default_brush(preset);
            let style = layer_render::DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
            let flags = batch_contact_flags(&style, &[body.into(), completion.into()], false);
            assert!(
                Reach::of(&brush)
                    .batch_variants(&style)
                    .any(|prepared| prepared & !AGGREGATE == flags)
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "Requires an explicitly selected hardware GPU"]
    fn fine_aggregate_pencil_color_pipeline_prepares_on_hardware() {
        let dxc = std::env::current_exe().unwrap().with_file_name("dxcompiler.dll");
        assert!(dxc.is_file(), "bundled DXC required: {dxc:?}");
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
        descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::DynamicDxc {
            dxc_path: dxc.to_str().unwrap().to_owned(),
        };
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env(&instance, None)).unwrap();
        let features = rendering_features(&adapter) | adapter.features()
            & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: memory_hints(),
            ..Default::default()
        })).unwrap();
        let renderer = WgpuRasterizer::from_wgpu_inner(adapter, device.into(), queue, Initialization::Snapshot).unwrap();
        let info = renderer.adapter.get_info();
        assert_ne!(info.device_type, wgpu::DeviceType::Cpu, "{info:?}");
        let material = &renderer.pipelines.dry_material;
        assert_eq!(OPERATIONS[0], MaterialOperation::Deposit);
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        brush.rendering.blend_mode = BrushBlendMode::Multiply;
        let style = layer_render::DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
        let (flags, blend) = material_entries(material_slot(&style));
        assert_eq!((flags, blend), (u32::MAX, 1));
        let cases = [
            (0x6843, MaterialOperation::Deposit, false, 0,
                &material.variants.get(&0x6843).expect("fine aggregate Pencil recipe")[0]),
            (0x6843 | COMPLETION, MaterialOperation::Deposit, false, 0,
                &material.variants.get(&(0x6843 | COMPLETION)).expect("fine aggregate Pencil completion recipe")[0]),
            (0x7043 | COMPLETION, MaterialOperation::Deposit, false, 0,
                &material.variants.get(&(0x7043 | COMPLETION)).expect("fine aggregate rounded completion recipe")[0]),
            (flags, MaterialOperation::Coverage, false, blend,
                material.selected_kernel(&style, MaterialOperation::Coverage, false, 0)),
            (flags, MaterialOperation::Coverage, true, blend,
                material.selected_kernel(&style, MaterialOperation::Coverage, true, 0)),
        ];
        eprintln!("adapter={info:?} features={:?} in_place={} filterable={} cache=none",
            renderer.device.features(), material.in_place(), renderer.device.features().contains(wgpu::Features::FLOAT32_FILTERABLE));
        let mut failures = Vec::new();
        for (contact, operation, coverage, blend, pipeline) in cases {
            let entry = if coverage { "compute_coverage" } else { "compute_color" };
            assert!(!pipeline.ready(), "contact={contact:#x} operation={operation:?} entry={entry} blend={blend}");
            eprintln!("BEGIN contact={contact:#x} operation={operation:?} entry={entry} blend={blend}");
            let internal_scope = renderer.device.push_error_scope(wgpu::ErrorFilter::Internal);
            let validation_scope = renderer.device.push_error_scope(wgpu::ErrorFilter::Validation);
            pollster::block_on(renderer.compilation.prepare_all([pipeline])).unwrap();
            let validation = pollster::block_on(validation_scope.pop());
            let internal = pollster::block_on(internal_scope.pop());
            eprintln!("END contact={contact:#x} operation={operation:?} entry={entry} blend={blend} validation={validation:?}; internal={internal:?}");
            if validation.is_some() || internal.is_some() {
                failures.push(format!("contact={contact:#x} operation={operation:?} entry={entry} blend={blend} validation={validation:?}; internal={internal:?}"));
            }
            assert!(pipeline.ready(), "contact={contact:#x} operation={operation:?} entry={entry} blend={blend}");
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn selecting_a_brush_with_another_blend_compiles_only_its_blend_kernels() {
        use layer_core::DefaultBrushPreset as Preset;
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let document = layer_core::Document::new(layer_core::authored::PortableId::random(), 128, 128,
            layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        for preset in [Preset::GPen, Preset::Pencil, Preset::Airbrush, Preset::Paintbrush, Preset::Charcoal, Preset::BristlePaintbrush,
            Preset::MultiplyGlaze, Preset::CloneStamp, Preset::HealingBrush] {
            for blend in [BrushBlendMode::Multiply, BrushBlendMode::Overlay] {
                let mut brush = layer_core::default_brush(preset);
                brush.rendering.blend_mode = blend;
                let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
                renderer.shader_idle(false, false);
                renderer.prepare_startup(&document, &brush, false).unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                crate::test_support::wait_startup(&mut renderer, deadline, |progress| progress.brush_ready, format_args!("{preset:?} preparation timed out"));
                let p = &renderer.pipelines.dry_material;
                let style = layer_render::DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
                let plan = BrushPassPlan::for_device(&style, &renderer.device);
                let ready = |kernels: &Kernels| kernels.iter().filter(|k| k.ready()).count();
                let blended = p.blended.values().map(ready).sum::<usize>();
                let copied = if p.in_place() {1+usize::from(plan.state.coverage)}
                    else {usize::from(plan.material == MaterialOperation::Deposit && dry_flow(&style))};
                println!("{preset:?} {blend:?}: {blended} blend kernels, copy {}", p.copy_kernel(false).ready());
                assert_eq!(blended, 1 + usize::from(plan.state.coverage), "{preset:?} {blend:?} compiles its commit and prediction kernels");
                assert!(p.selected_kernel(&style, plan.material, plan.state.coverage, 0).ready() && p.selected_kernel(&style, plan.material, false, 0).ready());
                assert_eq!(ready(&p.kernels) + p.variants.values().map(ready).sum::<usize>() + usize::from(p.gather.ready()), 0,
                    "{preset:?} {blend:?} compiles no normal kernel");
                assert_eq!(p.copy.iter().filter(|k| k.ready()).count(), copied, "{preset:?} {blend:?} prepares the unchanged-pixel copies its prediction binds");
            }
        }
    }

    #[test]
    fn blend_specialization_accepts_lf_and_crlf_sources() {
        let lf = include_str!("blend_modes.wgsl").replace("\r\n", "\n");
        let crlf = lf.replace('\n', "\r\n");
        for mode in MATERIAL_BLENDS.map(|blend| layer_core::LayerBlend::from(blend).code()) {
            let expected = blend_source_from(&lf, mode);
            for source in [&lf, &crlf] {
                let blend = blend_source_from(source, mode);
                assert_eq!(blend.replace("\r\n", "\n"), expected.replace("\r\n", "\n"));
                let source = format!("{}\n{}\n{}", working_color::source(Default::default()), include_str!("blend_codes.wgsl"), blend);
                let module = naga::front::wgsl::parse_str(&source).unwrap();
                naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                    .validate(&module).unwrap();
            }
        }
    }

    fn assert_no_member_after_a_bool(module: &naga::Module) {
        let boolean = |member: &naga::StructMember| matches!(module.types[member.ty].inner, naga::TypeInner::Scalar(naga::Scalar::BOOL));
        for (_, ty) in module.types.iter() {
            if let naga::TypeInner::Struct { members, .. } = &ty.inner {
                let first = members.iter().position(boolean).unwrap_or(members.len());
                assert!(members[first..].iter().all(boolean), "struct {:?} has a member after a bool", ty.name);
            }
        }
    }

    fn assert_no_bool_struct_results_in_loops(module: &naga::Module) {
        fn visit(module: &naga::Module, block: &naga::Block, in_loop: bool) {
            for statement in block.iter() {
                match statement {
                    naga::Statement::Block(block) => visit(module, block, in_loop),
                    naga::Statement::If { accept, reject, .. } => {
                        visit(module, accept, in_loop);
                        visit(module, reject, in_loop);
                    }
                    naga::Statement::Switch { cases, .. } => {
                        for case in cases { visit(module, &case.body, in_loop); }
                    }
                    naga::Statement::Loop { body, continuing, .. } => {
                        visit(module, body, true);
                        visit(module, continuing, true);
                    }
                    naga::Statement::Call { function, result: Some(_), .. } if in_loop => {
                        let function = &module.functions[*function];
                        if let Some(result) = &function.result {
                            if let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner {
                                assert!(members.iter().all(|member| !matches!(module.types[member.ty].inner,
                                    naga::TypeInner::Scalar(naga::Scalar::BOOL)
                                        | naga::TypeInner::Vector { scalar: naga::Scalar::BOOL, .. })),
                                    "loop calls {:?} returning a struct with bool members", function.name);
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        for (_, function) in module.functions.iter() { visit(module, &function.body, false); }
        for entry in &module.entry_points { visit(module, &entry.function.body, false); }
    }

    /// Browsers reject `a || b && c`, which naga accepts.
    fn assert_logical_operators_grouped(source: &str) {
        let mut groups = vec![[false; 2]];
        for (number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap().as_bytes();
            for (index, &byte) in code.iter().enumerate() {
                let pair = code.get(index + 1) == Some(&byte);
                match byte {
                    b'(' => groups.push([false; 2]),
                    b')' if groups.len() > 1 => { groups.pop(); }
                    b';' | b'{' | b'}' => groups = vec![[false; 2]],
                    b'&' if pair => groups.last_mut().unwrap()[0] = true,
                    b'|' if pair => groups.last_mut().unwrap()[1] = true,
                    _ => {}
                }
                assert!(groups.last().unwrap() != &[true; 2], "line {}: `{}` mixes || and && without parentheses", number + 1, line.trim());
            }
        }
    }

    #[test]
    fn every_kernel_entry_resolves_its_overrides() {
        let validate = |module: &naga::Module| {
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                .validate(module).unwrap();
        };
        let mut families=std::collections::BTreeSet::from([0,u32::MAX]);
        for preset in layer_core::CONTACT_BRUSH_PRESETS {
            let flags=contact_flags(layer_core::default_brush(preset).contact);
            families.extend([flags,flags|128,flags|8192,flags|16384,flags|8192|16384]);
            if flags&64!=0 {families.extend([flags|COMPLETION,flags|COMPLETION|AGGREGATE,
                flags|COMPLETION|FINE_TIP,flags|COMPLETION|FINE_TIP|AGGREGATE]);}
        }
        for &flags in &families {for operation in OPERATIONS {for filterable in [true, false] {
            for start in [Start::Stored, Start::Sampled] {
            let source = material_source(Default::default(),crate::pipeline_device::float_sampling_wgsl(filterable),Entries::Kernel(start, 0),flags,operation);
            assert_logical_operators_grouped(&source);
            let module = naga::front::wgsl::parse_str(&source).unwrap();
            assert_no_member_after_a_bool(&module);
            assert_no_bool_struct_results_in_loops(&module);
            assert_eq!(module.overrides.iter().any(|(_,v)|v.name.as_deref()==Some("CONTACT_FLAGS")),flags==u32::MAX);
            if flags!=u32::MAX {assert!(source.contains(&format!("const CONTACT_FLAGS: u32 = {flags}u;")));}
            assert!(!module.overrides.iter().any(|(_,v)|v.name.as_deref()==Some("MATERIAL_OPERATION")));
            assert!(source.contains(&format!("const MATERIAL_OPERATION: u32 = {}u;",operation as u32)));
            validate(&module);
            for entry in ["compute_color", "compute_coverage", "copy_color", "gather_display"] {
                let mut used = module.clone();
                used.entry_points.retain(|e| e.name == entry);
                assert_eq!(used.entry_points.len(), 1);
                naga::compact::compact(&mut used, naga::compact::KeepUnused::No);
                assert!(used.overrides.iter().all(|(_, value)| value.init.is_some()),
                    "{entry}/{operation:?}/{flags:#x} needs an override no pipeline supplies");
            }
        }}}}
        use crate::destination_material as destination;
        for slot in 0..MATERIAL_SLOTS {for filterable in [true, false] {
            let sampling = crate::pipeline_device::float_sampling_wgsl(filterable);
            let (flags, blend) = material_entries(slot);
            let pointwise = [Start::Stored, Start::Sampled].into_iter().flat_map(|start| OPERATIONS.into_iter()
                .flat_map(move |operation| ["compute_color", "compute_coverage"].map(|entry| (Entries::Kernel(start, blend), operation, entry))));
            let kernels = destination::OPERATIONS.into_iter()
                .filter(|&operation| destination::blended(operation) || slot % MATERIAL_BLENDS.len() == 0)
                .flat_map(|operation| destination::entries(operation).iter().flat_map(move |&entry| destination::blenders(operation).into_iter()
                    .map(move |blender| (destination::module(operation, blend, blender), operation, entry.name()))));
            for (entries, operation, entry) in pointwise.chain(kernels) {
                let source = material_source(Default::default(), sampling, entries, flags, operation);
                assert_logical_operators_grouped(&source);
                let mut used = naga::front::wgsl::parse_str(&source).unwrap();
                assert_no_member_after_a_bool(&used);
                assert_no_bool_struct_results_in_loops(&used);
                validate(&used);
                used.entry_points.retain(|e| e.name == entry);
                assert_eq!(used.entry_points.len(), 1, "{entry}/{operation:?}");
                naga::compact::compact(&mut used, naga::compact::KeepUnused::No);
                assert!(used.overrides.iter().all(|(_, value)| value.init.is_some()),
                    "{entry}/{operation:?}/{flags:#x} needs an override no pipeline supplies");
            }
        }}
    }

    #[test]
    fn specialized_modules_omit_unreachable_features() {
        use std::collections::BTreeSet;
        let names = |files: &[&str], entries: &[&str]| -> BTreeSet<String> {
            files.iter().flat_map(|file| file.lines()).filter_map(|line| line.strip_prefix("fn "))
                .filter_map(|line| line.split('(').next()).filter(|name| !entries.contains(name)).map(str::to_string).collect()
        };
        let compiled = |entries, flags, operation, entry: &str| {
            let source = material_source(Default::default(), crate::pipeline_device::float_sampling_wgsl(true), entries, flags, operation);
            let mut module = naga::front::wgsl::parse_str(&source).unwrap();
            module.entry_points.retain(|e| e.name == entry);
            naga::compact::compact(&mut module, naga::compact::KeepUnused::No);
            module.functions.iter().filter_map(|(_, f)| f.name.clone())
                .chain(module.global_variables.iter().filter_map(|(_, v)| v.name.clone())).collect::<BTreeSet<_>>()
        };
        let relief = names(&[include_str!("relief_transfer.wgsl"),include_str!("relief.wgsl"), include_str!("relief_fine.wgsl")], &["relief_contact"]);
        let completion = names(&[include_str!("relief_completion.wgsl")], &["relief_contact"]);
        let fine_completion = names(&[include_str!("relief_completion_fine.wgsl")], &["relief_contact"]);
        let bristle = names(&[include_str!("bristle.wgsl")], &["bristles_enabled", "bristle_footprint"]);
        let clone = names(&[include_str!("retouch_sample.wgsl"), include_str!("material_clone.wgsl")], &["clone_source_at"]);
        let blend = names(&[include_str!("blend_modes.wgsl")], &["blend"]);
        let pointwise = names(&[include_str!("material_pointwise.wgsl")], &[]);
        let fragments = ["smudge_fragment", "wet_fragment", "watercolor_fragment"];
        let absent = |present: &BTreeSet<String>, feature: &BTreeSet<String>, what: &str| {
            let found: Vec<_> = present.intersection(feature).collect();
            assert!(found.is_empty(), "{what} reaches {found:?}");
        };
        let retouching = [layer_core::DefaultBrushPreset::CloneStamp, layer_core::DefaultBrushPreset::HealingBrush];
        for preset in layer_core::CONTACT_BRUSH_PRESETS.into_iter().chain(retouching) {
            let brush = layer_core::default_brush(preset);
            let uniform = brush.rendering.accumulation == BrushAccumulation::Uniform;
            let flags = contact_flags(brush.contact) | if uniform { 128 } else { 0 };
            let operation = if retouching.contains(&preset) { MaterialOperation::Clone } else if uniform { MaterialOperation::Coverage } else { MaterialOperation::Deposit };
            for extra in [0, AGGREGATE, FINE_TIP] {
                let present = compiled(Entries::Kernel(Start::Stored, 0), flags | extra, operation, "compute_color");
                let what = format!("{preset:?} kernel {:#x}", flags | extra);
                if flags & 64 == 0 { absent(&present, &relief, &what); }
                absent(&present, &completion, &what);
                absent(&present, &fine_completion, &what);
                if flags & 512 == 0 { absent(&present, &bristle, &what); }
                if operation != MaterialOperation::Clone { absent(&present, &clone, &what); }
                if extra == AGGREGATE { absent(&present, &BTreeSet::from(["brush_selection".to_string()]), &what); }
                absent(&present, &blend, &what);
            }
            if flags&64!=0 && !uniform {
                let present=compiled(Entries::Kernel(Start::Stored,0),flags|COMPLETION,operation,"compute_color");
                absent(&present,&fine_completion,&format!("{preset:?} broad completion"));
            }
        }
        use crate::destination_material::{self as destination, Entry};
        for flags in [0, 128] {for operation in destination::OPERATIONS {for &entry in destination::entries(operation) {
            let what = format!("{operation:?} {entry:?} {flags:#x}");
            let present = compiled(destination::module(operation, 0, destination::PLAIN), flags, operation, entry.name());
            for feature in [&relief, &bristle, &clone, &pointwise, &blend] { absent(&present, feature, &what); }
            let operations: Vec<_> = fragments.iter().filter(|f| present.contains(**f)).collect();
            assert_eq!(operations.len(), usize::from(!matches!(entry, Entry::Reservoir)), "{what} reaches {operations:?}");
        }}}
        let multiply = layer_core::LayerBlend::from(BrushBlendMode::Multiply).code();
        let formulas = blend.iter().filter(|f| *f != "blend_luma").cloned().collect();
        for start in [Start::Stored, Start::Sampled] {
            let present = compiled(Entries::Kernel(start, multiply), 0, MaterialOperation::Deposit, "compute_color");
            assert!(present.contains("blend"), "{start:?} Multiply kernel blends");
            absent(&present, &formulas, "Multiply kernel");
        }
    }

    #[test]
    fn thin_filters_bound_contact_evaluations() {
        fn visit(
            block: &naga::Block,
            callee: naga::Handle<naga::Function>,
            in_loop: bool,
            counts: &mut [usize; 3],
            guards: &mut Vec<naga::Handle<naga::Expression>>,
        ) {
            for statement in block {
                match statement {
                    naga::Statement::Call { function, .. } if *function == callee => {
                        counts[0] += 1;
                        counts[2] += usize::from(in_loop);
                    }
                    naga::Statement::Block(body) => visit(body, callee, in_loop, counts, guards),
                    naga::Statement::If {
                        condition,
                        accept,
                        reject,
                    } => {
                        if in_loop && reject.iter().any(|s| matches!(s, naga::Statement::Break)) {
                            guards.push(*condition);
                        }
                        visit(accept, callee, in_loop, counts, guards);
                        visit(reject, callee, in_loop, counts, guards);
                    }
                    naga::Statement::Switch { cases, .. } => {
                        for case in cases {
                            visit(&case.body, callee, in_loop, counts, guards);
                        }
                    }
                    naga::Statement::Loop {
                        body, continuing, ..
                    } => {
                        counts[1] += 1;
                        visit(body, callee, true, counts, guards);
                        visit(continuing, callee, true, counts, guards);
                    }
                    _ => {}
                }
            }
        }

        let flags = contact_flags(
            layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil).contact,
        ) | AGGREGATE
            | FINE_TIP
            | COMPLETION;
        let source = material_source(
            Default::default(),
            crate::pipeline_device::float_sampling_wgsl(false),
            Entries::Kernel(Start::Stored, 0),
            flags,
            MaterialOperation::Deposit,
        );
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        for (name, callee, expected) in [
            ("relief_supersampled", "relief_evaluate", [2, 4]),
            ("relief_contact", "completion_exposure", [1, 5]),
        ] {
            let (_, function) = module
                .functions
                .iter()
                .find(|(_, f)| f.name.as_deref() == Some(name))
                .expect(name);
            let (callee, _) = module
                .functions
                .iter()
                .find(|(_, f)| f.name.as_deref() == Some(callee))
                .expect(callee);
            let count = *function
                .named_expressions
                .iter()
                .find(|(_, name)| name.as_str() == "count")
                .expect("bounded sample count")
                .0;
            let naga::Expression::Select { accept, reject, .. } = function.expressions[count]
            else {
                panic!("{name}: sample count must have two fixed bounds");
            };
            let literal = |expression| match function.expressions[expression] {
                naga::Expression::Literal(naga::Literal::U32(value)) => value,
                _ => panic!("{name}: sample limit must be a literal"),
            };
            let mut limits = [literal(accept), literal(reject)];
            limits.sort_unstable();
            assert_eq!(limits, expected, "{name}: contact evaluation budget");
            let mut counts = [0; 3];
            let mut guards = Vec::new();
            visit(&function.body, callee, false, &mut counts, &mut guards);
            assert_eq!(counts, [1, 1, 1], "{name}: one call site inside one loop");
            assert!(guards.iter().any(|&guard| matches!(function.expressions[guard],
                naga::Expression::Binary { op: naga::BinaryOperator::Less, right, .. } if right == count)),
                "{name}: the loop must use the bounded sample count");
        }
    }
}
